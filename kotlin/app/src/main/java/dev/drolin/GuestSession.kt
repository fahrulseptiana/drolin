package dev.drolin

import android.content.Context
import com.termux.terminal.TerminalSession
import com.termux.view.TerminalView
import java.io.File
import java.util.zip.GZIPInputStream

/**
 * Owns the one Termux terminal session that carries the guest console.
 *
 * Termux's TerminalSession creates a pty and runs the Rust engine on its slave
 * side. The engine is started with `--inherit`, so UML's `con0=fd:0,fd:1` gets
 * that same pty and the guest sees a real terminal.
 */
object GuestSession {
    @Volatile
    private var session: TerminalSession? = null
    private var view: TerminalView? = null

    @Volatile
    private var starting = false

    @Volatile
    private var finished = false

    val isFinished: Boolean
        get() = finished

    val isRunning: Boolean
        get() = session != null

    fun attach(activity: MainActivity, terminalView: TerminalView, client: TerminalClient) {
        view = terminalView
        terminalView.setTerminalViewClient(client)
        val current = session ?: return
        current.updateTerminalSessionClient(client)
        current.title
        terminalView.post {
            terminalView.attachSession(current)
            terminalView.requestFocus()
        }
    }

    fun detach() {
        view = null
    }

    fun start(activity: MainActivity, terminalView: TerminalView, client: TerminalClient) {
        if (session != null) {
            attach(activity, terminalView, client)
            activity.onSessionState("guest already running")
            return
        }
        if (starting) return
        starting = true
        try {
            activity.onSessionState("preparing Ubuntu guest")
            val libDir = activity.applicationInfo.nativeLibraryDir
            val engine = File(libDir, "libdrolin.so")
            val kernel = File(libDir, "liblinux.so")
            if (!engine.canExecute()) {
                activity.onSessionState("engine missing: ${engine.absolutePath}")
                return
            }
            if (!kernel.isFile) {
                activity.onSessionState("kernel missing: ${kernel.absolutePath}")
                return
            }
            val stub = File(libDir, "libstub.so")
        val passt = File(libDir, "libpasst.so")

            val rootfs = File(activity.filesDir, "rootfs.ext4")
            if (!rootfs.isFile) installBundledRootfs(activity, rootfs)

            val args = mutableListOf(
                "drolin", "run", "--inherit",
                "--kernel", kernel.absolutePath,
                "--memory", GuestSettings.memoryMib(activity).toString(),
                "--cpus", GuestSettings.cpus(activity).toString(),
            )
            if (rootfs.isFile) {
                args += listOf("--rootfs", rootfs.absolutePath, "--init", "/sbin/init")
            } else {
                val initramfs = installAsset(activity, "initramfs.cpio.gz")
                args += listOf("--initramfs", initramfs.absolutePath)
            }
            if (stub.canExecute()) args += listOf("--stub-exe", stub.absolutePath)
            if (passt.canExecute()) args += listOf("--net", "--passt", passt.absolutePath)

            val env = arrayOf(
                "DROLIN_HOME=${activity.filesDir.absolutePath}",
                "HOME=${activity.filesDir.absolutePath}",
                "TMPDIR=${activity.cacheDir.absolutePath}",
                "PATH=/system/bin:/system/xbin",
                "TERM=xterm-256color",
                "LANG=C.UTF-8",
            )
            val argv = args.toTypedArray()

            activity.runOnUiThread {
                try {
                    createSession(activity, terminalView, client, engine, argv, env)
                } catch (error: Exception) {
                    starting = false
                    activity.onSessionState("boot failed: ${error.message}")
                }
            }
        } finally {
            // createSession clears this on success; leaving it set on a handled
            // failure would require a second tap after the message.
        }
    }

    private fun createSession(
        activity: MainActivity,
        terminalView: TerminalView,
        client: TerminalClient,
        engine: File,
        args: Array<String>,
        env: Array<String>,
    ) {
        view = terminalView
        terminalView.setTerminalViewClient(client)
        val started = TerminalSession(
            engine.absolutePath,
            activity.filesDir.absolutePath,
            args,
            env,
            4000,
            client,
        )
        session = started
        starting = false
        finished = false
        activity.onSessionState("booting Ubuntu 26.04")
        terminalView.post {
            terminalView.attachSession(started)
            terminalView.requestFocus()
            terminalView.postInvalidate()
        }
    }

    fun stop() {
        session?.finishIfRunning()
        session = null
        finished = false
    }

    fun onSessionFinished() {
        session = null
        finished = true
    }

    private fun installBundledRootfs(context: Context, target: File) {
        val assets = context.assets.list("")?.toSet() ?: emptySet()
        if ("rootfs.ext4.gz" !in assets) return
        val partial = File(context.filesDir, "rootfs.ext4.part")
        context.assets.open("rootfs.ext4.gz").use { raw ->
            GZIPInputStream(raw).use { gzip ->
                partial.outputStream().use { output -> gzip.copyTo(output) }
            }
        }
        check(partial.renameTo(target)) { "cannot install ${target.name}" }
    }

    private fun installAsset(context: Context, name: String): File {
        val target = File(context.filesDir, name)
        if (target.isFile && target.length() > 0) return target
        context.assets.open(name).use { input ->
            target.outputStream().use { output -> input.copyTo(output) }
        }
        return target
    }
}
