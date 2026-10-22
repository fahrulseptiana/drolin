package dev.drolin

import android.content.Context
import java.io.File
import java.io.RandomAccessFile

/** Persisted boot settings and the sparse rootfs growth operation. */
object GuestSettings {
    private const val PREFS = "drolin-guest"
    private const val KEY_CPUS = "cpus"
    private const val KEY_MEMORY = "memory_mib"

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
