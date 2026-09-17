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
import org.json.JSONObject

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
    private var policy = ""
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
                offered?.let { update(obj("type" to "offer","key" to it.path.absolutePath,"owned" to true)) }
                capture()
                while (!closed) { delay(15_000); capture() }
            } catch (e: Exception) { host.reportActionError("Recovery unavailable: ${e.message}") }
        }
    }
    private fun update(event:JSONObject):JSONObject? {
        val result=JSONObject(Native.recoveryUpdate(policy,event.toString()))
        policy=result.getString("state")
        val view=result.getJSONObject("update")
        val key=view.optString("offer").takeUnless{it.isEmpty()||it=="null"}
        candidate=offered?.path?.takeIf{it.absolutePath==key}
        working=view.getBoolean("busy")
        for(release in view.getJSONArray("release").values()) {
            offered?.takeIf{it.path.absolutePath==release}?.let{held->held.close();offered=null}
        }
        return view.objectOrNull("work")
    }
    private suspend fun observation():JSONObject = JSONObject(host.withNative{Native.query(it,obj("type" to "recovery_document").toString())})
    private fun execute(first:JSONObject?):Job? {
        if(first==null)return null
        return scope.launch { storage.withLock {
            var next:JSONObject?=first
            while(next!=null) {
                val work=next;var success=false
                try {
                    when(work.getJSONObject("kind").getString("type")) {
                        "capture" -> success=writeSnapshot()
                        "retire" -> {withContext(Dispatchers.IO){java.nio.file.Files.deleteIfExists(path.toPath())};success=true}
                        "retire_origin" -> {
                            val key=work.getJSONObject("kind").getString("key")
                            val held=checkNotNull(offered?.takeIf{it.path.absolutePath==key})
                            withContext(Dispatchers.IO){java.nio.file.Files.deleteIfExists(held.path.toPath())};success=true
                        }
                        "restore" -> {
                            val key=work.getJSONObject("kind").getString("key")
                            val held=checkNotNull(offered?.takeIf{it.path.absolutePath==key})
                            val task=host.withNative{Native.projectRecoveryTask(it,true)}
                            try {
                                withContext(Dispatchers.IO){Native.projectWork(task,ParcelFileDescriptor.open(held.path,ParcelFileDescriptor.MODE_READ_ONLY).detachFd(),0,0)}
                                host.withNative{Native.projectAdopt(it,task,"null")};host.documentChanged()
                                update(obj("type" to "observe","document" to observation(),"owned" to (owner!=null)))
                                success=true
                            }finally{withContext(NonCancellable+Dispatchers.IO){Native.projectFree(task)}}
                        }
                    }
                }catch(e:Exception){host.reportActionError("Recovery operation failed: ${e.message}")}
                next=update(obj("type" to "complete","token" to work.getLong("token"),"success" to success))
            }
        } }
    }
    /** The shared observation excludes provisional operations, but allows committed ink capture. */
    fun capture():Job? {
        if(!started||owner==null||closed)return null
        return scope.launch {
            try{execute(update(obj("type" to "observe","document" to observation(),"owned" to true)))?.join()}
            catch(e:Exception){host.reportActionError("Recovery copy could not be saved: ${e.message}")}
        }
    }
    private suspend fun writeSnapshot(): Boolean {
        val task = host.withNative { Native.projectRecoveryTask(it, false) }
        if (task == 0L) return false
        try { withContext(Dispatchers.IO) { Native.projectPublish(task, path.absolutePath) } }
        finally { withContext(NonCancellable + Dispatchers.IO) { Native.projectFree(task) } }
        return true
    }
    fun retire() { execute(update(obj("type" to "retire","discard_origin" to true))) }
    fun dismiss(discard:Boolean) { execute(update(obj("type" to "dismiss","discard" to discard))) }
    fun recover() { execute(update(obj("type" to "restore"))) }
    fun close() {
        closed = true
        execute(update(obj("type" to "close")))
        scope.launch { storage.withLock {
            withContext(Dispatchers.IO) { offered?.close(); offered = null; owner?.close(); owner = null }
            scope.cancel()
        } }
    }
}
