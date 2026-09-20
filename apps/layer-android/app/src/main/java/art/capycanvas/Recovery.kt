package art.capycanvas

import android.app.Application
import android.os.ParcelFileDescriptor
import androidx.compose.runtime.*
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

/** One shared policy/lease per drawing, one serialized immutable writer per
 * window. Captures are frozen on the owner before waiting for older writes. */
internal class RecoveryController(private val host: CanvasHost, application: Application) {
    companion object { @Volatile internal var directoryForTest: File? = null }
    private val directory = directoryForTest ?: File(application.filesDir, "raster-recovery")
    private val scope=CoroutineScope(SupervisorJob()+Dispatchers.Main.immediate)
    private val storage=Mutex()
    private val ownerLane=Mutex()
    private class Held(val path:File,val channel:FileChannel,val lock:FileLock):AutoCloseable {
        override fun close(){try{lock.release()}finally{channel.close()}}
    }
    private class Owner(val id:Long,val held:Held) {var policy="";var job:Job?=null}
    private val owners=mutableMapOf<Long,Owner>()
    private val origins=mutableMapOf<String,Held>()
    private val seen=mutableSetOf<String>()
    private var offered:Held?=null
    private var polling:Job?=null
    private var started=false
    private var closed=false
    var candidate by mutableStateOf<File?>(null);private set
    var working by mutableStateOf(false);private set
    private fun claim(file:File):Held? {
        val channel=FileChannel.open(File(file.parentFile,"${file.name}.lock").toPath(),StandardOpenOption.CREATE,StandardOpenOption.WRITE)
        val lock=try{channel.tryLock()}catch(_:OverlappingFileLockException){null}catch(e:Exception){channel.close();throw e}
        if(lock==null){channel.close();return null};return Held(file,channel,lock)
    }
    suspend fun ensureOwners() {
        if(!started||closed)return
        ownerLane.withLock {
            val view=JSONObject(host.drawingTabs.query(obj("op" to "view")))
            for(tab in view.array("tabs").objects()) {
                val id=tab.getLong("id")
                if(id !in owners) {
                    val held=withContext(Dispatchers.IO) {
                        check(directory.mkdirs()||directory.isDirectory){"Cannot create recovery storage"}
                        checkNotNull(claim(File(directory,"${UUID.randomUUID()}.capy")))
                    }
                    owners[id]=Owner(id,held)
                }
            }
        }
    }
    fun start() {
        if(started||closed)return;started=true
        polling=scope.launch {
            try {ensureOwners();capture()?.join();offerNext();while(!closed){delay(15_000);capture()?.join()}}
            catch(e:CancellationException){throw e}
            catch(e:Exception){host.reportActionError("Recovery unavailable: ${e.message}")}
        }
    }
    private fun update(owner:Owner,event:JSONObject):JSONObject? {
        val result=JSONObject(Native.recoveryUpdate(owner.policy,event.toString()));owner.policy=result.getString("state")
        val view=result.getJSONObject("update")
        for(key in view.array("release").values())origins.remove(key.toString())?.close()
        return view.objectOrNull("work")
    }
    private suspend fun freeze(owner:Owner,work:JSONObject?):Long {
        if(work?.getJSONObject("kind")?.getString("type")!="capture")return 0
        return host.withNative{Native.projectRecoveryFor(it,owner.id)}
    }
    private suspend fun execute(owner:Owner,first:JSONObject?):Job? {
        if(first==null)return owner.job
        val frozen=try{freeze(owner,first)}catch(e:Exception){update(owner,obj("type" to "complete","token" to first.getLong("token"),"success" to false));throw e}
        val job=scope.launch {
            storage.withLock {
                var work:JSONObject?=first;var task=frozen
                while(work!=null) {
                    val current=work;var success=false
                    try {
                        when(current.getJSONObject("kind").getString("type")) {
                            "capture"->{if(task!=0L){withContext(Dispatchers.IO){Native.projectPublish(task,owner.held.path.absolutePath)};success=true}}
                            "retire"->{withContext(Dispatchers.IO){java.nio.file.Files.deleteIfExists(owner.held.path.toPath())};success=true}
                            "retire_origin"->{val key=current.getJSONObject("kind").getString("key");val held=checkNotNull(origins[key]);withContext(Dispatchers.IO){java.nio.file.Files.deleteIfExists(held.path.toPath())};success=true}
                        }
                    }catch(e:Exception){host.reportActionError("Recovery operation failed: ${e.message}")}
                    finally{withContext(NonCancellable+Dispatchers.IO){if(task!=0L)Native.projectFree(task)};task=0}
                    work=update(owner,obj("type" to "complete","token" to current.getLong("token"),"success" to success))
                    if(work!=null)try{task=freeze(owner,work)}catch(e:Exception){update(owner,obj("type" to "complete","token" to work.getLong("token"),"success" to false));host.reportActionError("Recovery capture failed: ${e.message}");break}
                }
            }
        };owner.job=job;return job
    }
    fun capture():Job? {
        if(!started||closed)return null
        return scope.launch {
            try {
                ensureOwners()
                for(owner in owners.values.toList()) {
                    val observation=JSONObject(host.drawingTabs.query(obj("op" to "recovery","id" to owner.id)))
                    execute(owner,update(owner,obj("type" to "observe","document" to observation,"owned" to true)))?.join()
                }
            }catch(e:Exception){host.reportActionError("Recovery copy could not be saved: ${e.message}")}
        }
    }
    fun retire(id:Long=host.drawingTabs.selected,closedTab:Boolean=false):Job? {
        val owner=owners[id]?:return null
        return scope.launch {
            if(closedTab)update(owner,obj("type" to "close"))
            execute(owner,update(owner,obj("type" to "retire","discard_origin" to true)))?.join()
            if(closedTab){owners.remove(id);withContext(Dispatchers.IO){owner.held.close()}}
        }
    }
    private suspend fun offerNext() {
        if(closed||working||offered!=null)return
        while(host.drawingTabs.switching||host.documents.working||host.documents.picker!=null)delay(50)
        offered=withContext(Dispatchers.IO) {
            directory.listFiles().orEmpty().filter{it.extension=="capy"&&it.absolutePath !in seen&&owners.values.none {o->o.held.path==it}}
                .sortedByDescending{it.lastModified()}.firstNotNullOfOrNull(::claim)
        }
        offered?.let{seen.add(it.path.absolutePath);candidate=it.path}
    }
    fun dismiss(discard:Boolean) {
        val held=offered?:return;offered=null;candidate=null
        scope.launch {try{withContext(Dispatchers.IO){if(discard)java.nio.file.Files.deleteIfExists(held.path.toPath());held.close()};offerNext()}catch(e:Exception){host.reportActionError("Recovery operation failed: ${e.message}")}}
    }
    fun recover() {
        val held=offered?:return;if(working)return;working=true
        scope.launch {
            var task=0L;var transition=false;var adopted=false
            try {
                host.drawingTabs.waitReady();host.drawingTabs.trim()
                task=host.withNative{Native.projectRecoveryTask(it,true)}
                withContext(Dispatchers.IO){Native.projectWork(task,ParcelFileDescriptor.open(held.path,ParcelFileDescriptor.MODE_READ_ONLY).detachFd(),0,0)}
                host.drawingTabs.beforeAdopt(task);transition=true
                host.withNative{Native.projectAdopt(it,task,"null")};adopted=true
                ensureOwners()
                val id=JSONObject(host.drawingTabs.query(obj("op" to "view"))).getLong("selected")
                origins[held.path.absolutePath]=held;offered=null;candidate=null
                val owner=owners.getValue(id)
                execute(owner,update(owner,obj("type" to "adopted","key" to held.path.absolutePath)))?.join()
                capture()?.join()
            }catch(e:Exception){host.reportActionError("Recovery operation failed: ${e.message}")}
            finally {
                withContext(NonCancellable+Dispatchers.IO){if(task!=0L)Native.projectFree(task)}
                if(transition)host.drawingTabs.afterAdopt()
                working=false;if(adopted)offerNext()
            }
        }
    }
    fun close() {
        closed=true;polling?.cancel()
        val accepted=scope.coroutineContext.job.children.toList()
        for(owner in owners.values)update(owner,obj("type" to "close"))
        scope.launch {accepted.joinAll();owners.values.mapNotNull{it.job}.joinAll();storage.withLock {withContext(Dispatchers.IO){offered?.close();origins.values.forEach{it.close()};owners.values.forEach{it.held.close()}};scope.cancel()}}
    }
}
