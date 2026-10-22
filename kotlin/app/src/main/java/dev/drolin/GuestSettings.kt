package dev.drolin

import android.content.Context
import java.io.File
import java.io.RandomAccessFile

/** Persisted boot settings and the sparse rootfs growth operation. */
object GuestSettings {
    private const val PREFS = "drolin-guest"
    private const val KEY_CPUS = "cpus"
    private const val KEY_MEMORY = "memory_mib"
    private const val KEY_FORWARDS = "forwards"

    data class PortForward(val protocol: String, val hostPort: Int, val guestPort: Int) {
        override fun toString(): String = "$protocol $hostPort -> $guestPort"
        fun engineArg(): String = "$protocol:$hostPort:$guestPort"
    }

    private fun prefs(context: Context) = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    fun cpus(context: Context): Int = prefs(context).getInt(KEY_CPUS, 1).coerceIn(1, 8)

    fun memoryMib(context: Context): Int =
        prefs(context).getInt(KEY_MEMORY, 512).let { if (it < 128) 512 else it }

    fun setCpus(context: Context, value: Int) {
        prefs(context).edit().putInt(KEY_CPUS, value.coerceIn(1, 8)).apply()
    }

    fun setMemoryMib(context: Context, value: Int) {
        prefs(context).edit().putInt(KEY_MEMORY, value).apply()
    }

    fun forwards(context: Context): List<PortForward> =
        prefs(context).getString(KEY_FORWARDS, "").orEmpty()
            .lineSequence()
            .mapNotNull { line ->
                val parts = line.split(':')
                if (parts.size != 3) return@mapNotNull null
                val host = parts[1].toIntOrNull() ?: return@mapNotNull null
                val guest = parts[2].toIntOrNull() ?: return@mapNotNull null
                PortForward(parts[0], host, guest)
            }
            .toList()

    fun addForward(context: Context, protocol: String, hostPort: Int, guestPort: Int): Result<Unit> = runCatching {
        require(protocol == "tcp" || protocol == "udp") { "protocol must be tcp or udp" }
        require(hostPort in 1024..65535) { "host port must be 1024..65535" }
        require(guestPort in 1..65535) { "guest port must be 1..65535" }
        val next = forwards(context).toMutableList()
        require(next.none { it.protocol == protocol && it.hostPort == hostPort }) {
            "host port $hostPort/$protocol is already forwarded"
        }
        next += PortForward(protocol, hostPort, guestPort)
        prefs(context).edit()
            .putString(KEY_FORWARDS, next.joinToString("\n") { it.engineArg() })
            .apply()
    }

    fun removeForward(context: Context, forward: PortForward) {
        val next = forwards(context).filterNot { it == forward }
        prefs(context).edit()
            .putString(KEY_FORWARDS, next.joinToString("\n") { it.engineArg() })
            .apply()
    }

    fun rootfs(context: Context): File = File(context.filesDir, "rootfs.ext4")

    fun currentBytes(context: Context): Long = rootfs(context).let { if (it.isFile) it.length() else 0L }

    /**
     * Extend the backing file. The new blocks are sparse; systemd-growfs in the
     * guest resizes ext4 to the new device size on the next boot.
     */
    fun expandStorage(context: Context, targetGib: Int): Result<Long> = runCatching {
        val image = rootfs(context)
        require(image.isFile && image.length() > 0) { "no Ubuntu image installed" }
        val target = targetGib.toLong() * 1024L * 1024L * 1024L
        require(target > image.length()) {
            "already ${image.length() / (1024L * 1024L)} MiB; choose a larger size"
        }
        RandomAccessFile(image, "rw").use { it.setLength(target) }
        target
    }
}
