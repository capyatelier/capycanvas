package art.capycanvas

import android.app.Application
import android.os.ParcelFileDescriptor
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import kotlinx.coroutines.*
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import java.io.File
import java.nio.channels.FileChannel
import java.nio.channels.FileLock
import java.nio.channels.OverlappingFileLockException
import java.nio.file.StandardOpenOption
import java.util.UUID

/** One immutable capture in flight per window. File locks exclude live windows;
 * complete sibling-file publication is shared with GTK in Rust. */
internal class RecoveryController(private val host: CanvasHost, application: Application) {
    companion object { @Volatile internal var directoryForTest: File? = null }
    private val directory = directoryForTest ?: File(application.filesDir, "raster-recovery")
    private val path = File(directory, "${UUID.randomUUID()}.capy")
    // Unlike viewModelScope, this scope finishes an accepted immutable write
    // after Activity teardown. It never accesses the live App on the file worker.
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val storage = Mutex()
    private var owner: Held? = null
    private var offered: Held? = null
    private var started = false
    private var closed = false
    private var serial = 0L
    private var checkpoint: String? = null
    private var pending = false
    var candidate by mutableStateOf<File?>(null)
        private set
    var working by mutableStateOf(false)
        private set
    private class Held(val path: File, val channel: FileChannel, val lock: FileLock) : AutoCloseable {
        override fun close() { try { lock.release() } finally { channel.close() } }
    }
    private fun claim(file: File): Held? {
        val channel = FileChannel.open(File(file.parentFile, "${file.name}.lock").toPath(), StandardOpenOption.CREATE, StandardOpenOption.WRITE)
        val lock = try { channel.tryLock() }
            catch (_: OverlappingFileLockException) { null }
            catch (e: Exception) { channel.close(); throw e }
        if (lock == null) { channel.close(); return null }
        return Held(file, channel, lock)
    }
    fun start() {
        if (started || closed) return
        started = true
        scope.launch {
            try {
                storage.withLock { withContext(Dispatchers.IO) {
                    check(directory.mkdirs() || directory.isDirectory) { "Cannot create recovery storage" }
                    owner = checkNotNull(claim(path))
                    offered = directory.listFiles().orEmpty().filter { it.extension == "capy" && it != path }
                        .sortedByDescending { it.lastModified() }.firstNotNullOfOrNull(::claim)
                } }
                candidate = offered?.path
                while (!closed) { delay(15_000); capture() }
            } catch (e: Exception) { host.reportActionError("Recovery unavailable: ${e.message}") }
        }
    }
    /** Safe to request during a contact: capture_project_recovery excludes live ink. */
    fun capture(): Job? {
        if (!started || owner == null || closed || pending || working || candidate != null) return null
        val file = host.snapshot?.objectOrNull("state")?.objectOrNull("document_file") ?: return null
        if (file.optBoolean("busy")) return null
        val version = "${file.optLong("epoch")}:${file.optLong("revision")}:${file.optBoolean("modified")}"
        if (checkpoint == version) return null
        pending = true
        val generation = serial
        return scope.launch {
            try {
                storage.withLock {
                    if (generation != serial) return@withLock
                    if (file.optBoolean("modified")) writeSnapshot()
                    else withContext(Dispatchers.IO) { java.nio.file.Files.deleteIfExists(path.toPath()) }
                    checkpoint = version
                }
            } catch (e: Exception) { host.reportActionError("Recovery copy could not be saved: ${e.message}") }
            finally { pending = false }
        }
    }
    private suspend fun writeSnapshot() {
        val task = host.withNative { Native.projectRecoveryTask(it, false) }
        try { withContext(Dispatchers.IO) { Native.projectPublish(task, path.absolutePath) } }
        finally { withContext(NonCancellable + Dispatchers.IO) { Native.projectFree(task) } }
    }
    fun retire() {
        serial++; checkpoint = null
        scope.launch { storage.withLock {
            try { withContext(Dispatchers.IO) { java.nio.file.Files.deleteIfExists(path.toPath()) } }
            catch (e: Exception) { host.reportActionError("Previous recovery copy could not be removed: ${e.message}") }
        } }
    }
    fun dismiss(discard: Boolean) {
        if (working) return
        val held = offered ?: return
        offered = null; candidate = null
        scope.launch { withContext(Dispatchers.IO) {
            try { if (discard) java.nio.file.Files.deleteIfExists(held.path.toPath()) }
            finally { held.close() }
        } }
    }
    fun recover() {
        val held = offered ?: return
        if (working) return
        working = true
        scope.launch {
            var task = 0L
            try {
                task = host.withNative { Native.projectRecoveryTask(it, true) }
                withContext(Dispatchers.IO) {
                    Native.projectWork(task, ParcelFileDescriptor.open(held.path, ParcelFileDescriptor.MODE_READ_ONLY).detachFd(), 0, 0)
                }
                host.withNative { Native.projectAdopt(it, task, "null") }
                host.documentChanged()
                // Establish this window's complete copy before retiring the origin.
                storage.withLock { writeSnapshot() }
                withContext(Dispatchers.IO) { java.nio.file.Files.deleteIfExists(held.path.toPath()); held.close() }
                offered = null; candidate = null
            } catch (e: Exception) { host.reportActionError("Drawing could not be recovered: ${e.message}") }
            finally {
                if (task != 0L) withContext(NonCancellable + Dispatchers.IO) { Native.projectFree(task) }
                working = false
            }
        }
    }
    fun close() {
        closed = true
        scope.launch { storage.withLock {
            withContext(Dispatchers.IO) { offered?.close(); offered = null; owner?.close(); owner = null }
            scope.cancel()
        } }
    }
}
