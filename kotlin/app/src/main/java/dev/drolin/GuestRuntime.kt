package dev.drolin

import android.content.Context
import java.io.File

/**
 * Owns the guest process. The Activity draws it, the Service keeps the process
 * alive, and neither knows how a boot plan is built: that is the Rust engine's
 * job. This object only forwards bytes and state.
 */
object GuestRuntime {
    interface Listener {
        fun onOutput(text: String)
        fun onState(text: String)
    }

    private const val MAX_BUFFER = 200_000

    private val lock = Any()
    private val buffer = StringBuilder()
    private var process: Process? = null
    private var listener: Listener? = null

    val isRunning: Boolean
        get() = synchronized(lock) { process?.isAlive == true }

    fun attach(next: Listener) {
        val snapshot: String
        synchronized(lock) {
            listener = next
            snapshot = buffer.toString()
        }
        next.onOutput(snapshot)
        next.onState(if (isRunning) "guest running" else "idle")
    }

    fun detach(which: Listener) {
        synchronized(lock) {
            if (listener === which) listener = null
        }
    }

    fun start(context: Context) {
        if (isRunning) return
        synchronized(lock) { buffer.setLength(0) }

        val libDir = context.applicationInfo.nativeLibraryDir
        val engine = File(libDir, "libdrolin.so")
        val kernel = File(libDir, "liblinux.so")
        if (!engine.canExecute()) {
            emit("engine missing: $engine\nbuild the Rust engine for this ABI and reinstall\n")
            return
        }
        if (!kernel.isFile) {
            emit("kernel missing: $kernel\nbuild kernel/ and package it as liblinux.so\n")
            return
        }

        val initramfs = installAsset(context, "initramfs.cpio.gz")
        val log = File(context.filesDir, "guest.log")
        val stub = File(libDir, "libstub.so")
        val command = mutableListOf(
            engine.absolutePath,
            "run",
            "--kernel", kernel.absolutePath,
            "--initramfs", initramfs.absolutePath,
            "--memory", "512",
            "--marker", "DROLIN_GUEST_OK",
            "--timeout", "300",
            "--log", log.absolutePath,
        )
        if (stub.canExecute()) {
            command.add("--stub-exe")
            command.add(stub.absolutePath)
        }

        emit("$ ${command.joinToString(" ")}\n")
        emit("state: starting\n")
        val started = try {
            ProcessBuilder(command)
                .redirectErrorStream(true)
                .apply {
                    // UML needs a writable, executable temporary directory for
                    // its guard page and for the standalone-stub fallback.
                    // App cache is the one place an unprivileged app may mmap
                    // PROT_EXEC; /tmp and /dev/shm are not.
                    environment()["DROLIN_HOME"] = context.filesDir.absolutePath
                    environment()["TMPDIR"] = context.cacheDir.absolutePath
                    environment()["HOME"] = context.filesDir.absolutePath
                }
                .start()
        } catch (error: Exception) {
            emit("cannot start the engine: ${error.message}\n")
            return
        }

        synchronized(lock) { process = started }
        emit("state: running\n")

        Thread({ pump(started) }, "drolin-output").apply { isDaemon = true }.start()
        Thread({ reap(started) }, "drolin-reaper").apply { isDaemon = true }.start()
    }

    fun send(line: String) {
        val current = synchronized(lock) { process } ?: return
        try {
            current.outputStream.write((line + "\n").toByteArray())
            current.outputStream.flush()
        } catch (error: Exception) {
            emit("stdin closed: ${error.message}\n")
        }
    }

    fun stop() {
        val current = synchronized(lock) { process } ?: return
        emit("state: stopping\n")
        current.destroy()
    }

    private fun pump(current: Process) {
        try {
            current.inputStream.bufferedReader().useLines { lines ->
                lines.forEach { emit(it + "\n") }
            }
        } catch (error: Exception) {
            emit("output closed: ${error.message}\n")
        }
    }

    private fun reap(current: Process) {
        val code = try {
            current.waitFor()
        } catch (error: InterruptedException) {
            Thread.currentThread().interrupt()
            -1
        }
        synchronized(lock) {
            if (process === current) process = null
        }
        emit("state: stopped (exit $code)\n")
    }

    private fun installAsset(context: Context, name: String): File {
        val target = File(context.filesDir, name)
        if (target.isFile && target.length() > 0) return target
        context.assets.open(name).use { input ->
            target.outputStream().use { output -> input.copyTo(output) }
        }
        return target
    }

    private fun emit(text: String) {
        val current: Listener?
        synchronized(lock) {
            buffer.append(text)
            if (buffer.length > MAX_BUFFER) {
                buffer.delete(0, buffer.length - MAX_BUFFER)
            }
            current = listener
        }
        current?.onOutput(text)
    }
}
