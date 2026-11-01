package art.capycanvas

import java.io.File
import java.nio.channels.FileChannel
import java.nio.channels.FileLock
import java.nio.channels.OverlappingFileLockException
import java.nio.file.Files
import java.nio.file.NoSuchFileException
import java.nio.file.Path
import java.nio.file.StandardOpenOption
import java.nio.file.attribute.BasicFileAttributes

internal object SessionOwners {
    private val owners = mutableSetOf<Held>()
    class Held internal constructor(val path: File, internal val key: Path, internal val identity: Any, private val channel: FileChannel, val lock: FileLock): AutoCloseable {
        private var closed = false
        override fun close() = synchronized(owners) {
            if (!closed) {
                closed = true
                try { lock.release() } finally { try { channel.close() } finally { owners.remove(this) } }
            }
            Unit
        }
    }
    @JvmStatic fun claim(directory: String): AutoCloseable? = claim(File(directory))
    fun claim(path: File): Held? = synchronized(owners) {
        check(path.mkdirs() || path.isDirectory) { "Cannot create session storage" }
        val key = File(path, "owner.lock").canonicalFile.toPath()
        val identity = try { checkNotNull(Files.readAttributes(key, BasicFileAttributes::class.java).fileKey()) } catch (_: NoSuchFileException) { null }
        if (owners.any { it.key == key || identity != null && it.identity == identity }) return@synchronized null
        val channel = FileChannel.open(key, StandardOpenOption.CREATE, StandardOpenOption.WRITE)
        try {
            val lock = try { channel.tryLock() } catch (_: OverlappingFileLockException) { null }
            if (lock == null) { channel.close(); return@synchronized null }
            val held = Held(path, key, checkNotNull(Files.readAttributes(key, BasicFileAttributes::class.java).fileKey()), channel, lock)
            owners.add(held); held
        } catch (error: Exception) { channel.close(); throw error }
    }
}
