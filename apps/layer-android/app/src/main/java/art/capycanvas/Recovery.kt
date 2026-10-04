package art.capycanvas

import android.app.Application
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

internal class RecoveryController(private val host: CanvasHost, application: Application) {
    companion object { @Volatile internal var directoryForTest: File? = null }
    private val directory = directoryForTest ?: File(application.filesDir, "sessions")
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val storage = Mutex()
    private val publication = Mutex()
    private class Held(val path: File, val channel: FileChannel, val lock: FileLock): AutoCloseable {
        override fun close() { try { lock.release() } finally { channel.close() } }
    }
    private class Owner(val key: String) { var stamp = "" }
    private val owners = mutableMapOf<Long, Owner>()
    private var held: Held? = null
    private val retained = mutableListOf<Held>()
    private val stores = mutableMapOf<String, Long>()
    private var manifest = ""
    private var publicationPending = false
    private class PublicationFailure(val published: Boolean, message: String): IllegalStateException(message)
    private var polling: Job? = null
    private var checkpoint: Job? = null
    private var started = false
    private var closed = false
    private var backgrounded = false
    private var restoring = false
    private var startupStamp = "null"
    var candidate by mutableStateOf<File?>(null); private set
    var working by mutableStateOf(false); private set
    var ready by mutableStateOf(false); private set
    private suspend fun reportFailure(error: Exception, recovery: Boolean = false) {
        host.reportActionError(host.withNative {Native.sessionFailure(it,error.message.orEmpty(),recovery)})
    }
    private fun claim(path: File): Held? {
        check(path.mkdirs() || path.isDirectory) { "Cannot create session storage" }
        val channel = FileChannel.open(File(path, "owner.lock").toPath(), StandardOpenOption.CREATE, StandardOpenOption.WRITE)
        val lock = try { channel.tryLock() } catch (_: OverlappingFileLockException) { null }
            catch (e: Exception) { channel.close(); throw e }
        if (lock == null) { channel.close(); return null }
        return Held(path, channel, lock)
    }
    private suspend fun writeManifest(event: JSONObject) = publication.withLock {
        val next = Native.sessionManifestUpdate(manifest, event.toString())
        if(next == manifest&&!publicationPending)return@withLock
        val result = withContext(Dispatchers.IO) {JSONObject(Native.sessionManifestWrite(File(checkNotNull(held).path,"session.json").absolutePath,next))}
        if(result.getBoolean("published"))manifest = next
        publicationPending = !result.isNull("error")
        if(publicationPending)throw PublicationFailure(result.getBoolean("published"),result.getString("error"))
        manifest = next
        if(event.getString("type") == "reconcile")withContext(Dispatchers.IO) {Native.sessionCollect(checkNotNull(held).path.absolutePath,manifest)}
    }
    private suspend fun store(path: File): Long = withContext(NonCancellable) {
        stores[path.absolutePath] ?: withContext(Dispatchers.IO) {Native.sessionStoreOpen(path.absolutePath)}
            .also {stores[path.absolutePath] = it}
    }
    suspend fun ensureOwners() {
        if (!started || restoring) return
        val view = JSONObject(host.drawingTabs.query(obj("op" to "view")))
        for (tab in view.array("tabs").objects()) owners.getOrPut(tab.getLong("id")) { Owner(UUID.randomUUID().toString()) }
    }
    fun start() {
        if (started || closed) return
        started = true
        scope.launch {
            try {
                restoring = true
                startupStamp = host.withNative {Native.sessionStamp(it,host.drawingTabs.selected)}
                val abandoned = withContext(NonCancellable) {withContext(Dispatchers.IO) {
                    check(directory.mkdirs() || directory.isDirectory) { "Cannot create session storage" }
                    var selected: Held? = null
                    for(path in directory.listFiles().orEmpty().filter {it.isDirectory&&File(it,"session.json").isFile}
                        .sortedByDescending {File(it,"session.json").lastModified()}) {
                        val owner = claim(path) ?: continue
                        val saved = runCatching {Native.sessionManifestRead(File(path,"session.json").absolutePath)}.getOrNull()
                        val empty = saved != null&&JSONObject(saved).array("drawings").length() == 0
                        if(empty)try {Native.sessionCollect(path.absolutePath,checkNotNull(saved))}finally {owner.close()}
                        else if(selected == null)selected = owner else owner.close()
                    }
                    selected
                }.also {held=it}}
                if(abandoned == null)withContext(NonCancellable) {
                    held = withContext(Dispatchers.IO) {checkNotNull(claim(File(directory,UUID.randomUUID().toString())))}
                }
                if (abandoned != null) {
                    try {
                        manifest = withContext(Dispatchers.IO) { Native.sessionManifestRead(File(abandoned.path, "session.json").absolutePath) }
                        restoreAll()
                    } catch(e:CancellationException) { throw e }
                    catch(e:Exception) { preserveFailed(abandoned);reportFailure(e,true) }
                }
                restoring = false
                ensureOwners()
                capture()?.join()
                ready = true
                if(!closed)polling = scope.launch {while (!closed) { delay(2_000); capture()?.join() }}
            } catch (e: CancellationException) { throw e }
            catch (e: Exception) { restoring = false; ready = true; reportFailure(e) }
            finally {restoring=false}
        }
    }
    private suspend fun preserveFailed(source: Held) {
        candidate = source.path
        if(source !in retained)retained.add(source)
        held = withContext(Dispatchers.IO) { checkNotNull(claim(File(directory, UUID.randomUUID().toString()))) }
        manifest = ""; owners.clear()
    }
    private suspend fun save(cleanExit: Boolean): Boolean = storage.withLock {
        ensureOwners()
        val view = JSONObject(host.drawingTabs.query(obj("op" to "view")))
        val drawings = org.json.JSONArray(view.array("tabs").objects().map {
            obj("id" to it.getLong("id"), "key" to owners.getValue(it.getLong("id")).key)
        })
        writeManifest(obj("type" to "stage", "drawings" to drawings, "active" to view.getLong("selected")))
        for (tab in view.array("tabs").objects()) {
            val id = tab.getLong("id"); val owner = owners.getValue(id)
            val stamp = host.withNative { Native.sessionStamp(it, id) }
            if (owner.stamp == stamp) continue
            val task = host.withNative { Native.sessionCapture(it, id) }
            if(task == 0L)return@withLock false
            try { val backing = store(File(checkNotNull(held).path, owner.key)); withContext(Dispatchers.IO) { Native.sessionCommit(task, backing) }; owner.stamp = stamp }
            finally { withContext(NonCancellable + Dispatchers.IO) { Native.sessionFree(task) } }
        }
        val members = JSONObject(manifest).array("drawings")
        val complete = members.length() == drawings.length()
        writeManifest(obj("type" to "reconcile", "drawings" to if(complete)drawings else members,
            "active" to view.getLong("selected"), "clean_exit" to cleanExit))
        true
    }
    fun capture(): Job? {
        if (!started || closed || backgrounded || restoring || held == null || host.drawingTabs.switching) return null
        checkpoint?.takeIf { it.isActive }?.let { return it }
        return scope.launch {
            try { save(false) }
            catch (e: CancellationException) { throw e }
            catch (e: Exception) {reportFailure(e)}
        }.also { checkpoint = it }
    }
    fun foreground() {backgrounded=false;capture()}
    fun background() {
        backgrounded=true
        if(!closed&&!host.drawingTabs.closingWindow)scope.launch {if(flush()&&!backgrounded)capture()}
    }
    suspend fun flush(): Boolean = protect(true)
    suspend fun checkpointForSave(): Boolean = protect(false)
    private suspend fun protect(cleanExit: Boolean): Boolean {
        if (!started || closed) return false
        return try {
            while (restoring&&!closed) delay(20)
            if(closed)return false
            checkpoint?.join()
            withTimeout(30_000) { while(!save(cleanExit)) { host.documentChanged(); delay(16) } }
            true
        } catch (e: CancellationException) { throw e }
        catch (e: Exception) {reportFailure(e);false}
    }
    suspend fun remove(id: Long): Unit = storage.withLock {
        owners[id]?.let {owner -> stores[File(checkNotNull(held).path,owner.key).absolutePath] }?.let {backing ->
            withContext(Dispatchers.IO) {Native.sessionStorePrepareRetirement(backing)}
        }
        try {writeManifest(obj("type" to "remove", "id" to id))}
        catch(e:PublicationFailure) {
            if(!e.published)throw e
            reportFailure(e)
        }
        Unit
    }
    suspend fun closed(id: Long): Unit = storage.withLock {
        owners.remove(id)?.let {owner ->
            stores.remove(File(checkNotNull(held).path,owner.key).absolutePath)?.let {backing ->
                try {withContext(Dispatchers.IO) {Native.sessionStoreRetire(backing)}}
                catch(e:Exception) {reportFailure(e)}
                finally {withContext(NonCancellable+Dispatchers.IO) {Native.sessionStoreFree(backing)}}
            }
        }
        Unit
    }
    private suspend fun restoreAll(retry: Boolean = false): Boolean {
        val source = checkNotNull(held)
        if(retry) {
            val mapping = JSONObject(manifest).array("drawings").objects().mapNotNull {row ->
                owners.entries.firstOrNull {it.value.key == row.getString("key")}
                    ?.takeIf {it.key != row.getLong("id")}
                    ?.let {org.json.JSONArray(listOf(row.getLong("id"),it.key))}
            }
            if(mapping.isNotEmpty())writeManifest(obj("type" to "remap","mapping" to org.json.JSONArray(mapping)))
        }
        val original = JSONObject(manifest)
        val allRows = original.array("drawings").objects()
        val liveSource = retry && owners.values.any {owner -> allRows.any {it.getString("key") == owner.key}}
        val retryIds = original.array("blocked").values().map {it.toString().toLong()}
        val rows = if(liveSource)allRows.filter {it.getLong("id") in retryIds} else allRows
        val active = original.getLong("active")
        var adopted = liveSource
        var appended = false
        val ids = mutableMapOf<Long,Long>()
        val pendingPoll = scope.launch {
            while(!closed) { delay(2_000); capture()?.join() }
        }
        working = true
        try {
            writeManifest(obj("type" to "interrupted"))
            val blocked = JSONObject(manifest).array("blocked").values().map { it.toString().toLong() }
            for(row in rows.sortedBy {if(it.getLong("id") == active)0 else 1}) {
                val id = row.getLong("id")
                if(id in blocked && !retry) {candidate = source.path;continue}
                var task = 0L;var transition = false;var attempt: JSONObject? = null
                try {
                    writeManifest(obj("type" to if(id in blocked) "retry_restore" else "begin_restore","id" to id))
                    attempt = JSONObject(manifest).array("restoring").objects().first {it.getLong("id") == id}.copy()
                    if(!adopted)host.drawingTabs.waitReady(settle={closed})
                    task = withContext(NonCancellable) {host.withNative {Native.sessionRestoreTask(it,if(retry)"null" else startupStamp)}}
                    val backing = store(File(source.path,row.getString("key")))
                    withContext(Dispatchers.IO) {
                        val location = Native.sessionRead(task,backing,!original.getBoolean("clean_exit"))
                        val observed = host.documents.observeDestination(location.takeUnless {it == "null"}?.let {JSONObject(it).getString("uri")},task)
                        Native.sessionPrepare(task,id,observed)
                    }
                    if(!adopted) {
                        host.drawingTabs.beforeSessionAdopt(settle={closed});transition = true
                        val result = JSONObject(host.withNative {Native.sessionAdopt(it,task,id,org.json.JSONArray(allRows.map {it.getLong("id")}).toString())})
                        appended = result.getBoolean("preserved")
                        ids[id] = result.getJSONObject("ids").getLong(id.toString())
                        adopted = true
                    } else ids[id] = host.withNative {Native.sessionHydrate(it,task,id)}
                    owners[ids.getValue(id)] = Owner(row.getString("key"))
                    writeManifest(obj("type" to "finish_restore","attempt" to attempt,"success" to true))
                    if(!appended) {restoring = false;ready = true}
                    host.documentChanged()
                } catch(e:CancellationException) {throw e}
                catch(e:Exception) {
                    if(attempt != null)writeManifest(obj("type" to "finish_restore","attempt" to attempt,"success" to false))
                    candidate = source.path
                    reportFailure(e,true)
                } finally {
                    withContext(NonCancellable+Dispatchers.IO) {if(task != 0L)Native.sessionFree(task)}
                    if(transition)host.drawingTabs.afterAdopt()
                }
            }
            if(!adopted) {
                val live = JSONObject(host.drawingTabs.query(obj("op" to "view"))).array("tabs").objects().map {it.getLong("id")}.toSet()
                var next = (allRows.map {it.getLong("id")}+live).maxOrNull() ?: 0L
                val mapping = allRows.filter {it.getLong("id") in live}.map {row ->
                    check(next < Long.MAX_VALUE) {"Drawing identities are exhausted"}
                    org.json.JSONArray(listOf(row.getLong("id"),++next))
                }
                if(mapping.isNotEmpty())writeManifest(obj("type" to "remap","mapping" to org.json.JSONArray(mapping)))
                val reserved = JSONObject(manifest).array("drawings").objects().map {it.getLong("id")}
                host.withNative {Native.sessionReserveIdentities(it,org.json.JSONArray(reserved).toString())}
                candidate = source.path
                return false
            }
            for(row in rows)ids[row.getLong("id")]?.let {host.drawingTabs.query(obj("op" to "reorder","id" to it,"before" to null))}
            if(appended) {
                val mapping = ids.filter {it.key != it.value}.map {org.json.JSONArray(listOf(it.key,it.value))}
                if(mapping.isNotEmpty())writeManifest(obj("type" to "remap","mapping" to org.json.JSONArray(mapping)))
            }
            return candidate == null
        } finally {pendingPoll.cancel();working = false}
    }
    fun recover() {
        val path = candidate ?: return
        if(working||closed)return
        working = true
        scope.launch {
            storage.withLock {
                restoring = true
                val current = checkNotNull(held)
                val preserved = if(current.path == path)current else retained.firstOrNull { it.path == path }
                    ?: run {restoring=false;working=false;return@withLock}
                var previous = manifest
                val previousOwners = owners.toMap()
                var ownsSource = current === preserved
                try {
                    if(current !== preserved) {
                        host.drawingTabs.waitReady()
                        host.withNative {Native.sessionRequireEmptyRetry(it)}
                        for(id in JSONObject(manifest).array("drawings").objects().map {it.getLong("id")}) {
                            owners[id]?.let {owner ->store(File(current.path,owner.key))}?.let {backing ->
                                withContext(Dispatchers.IO) {Native.sessionStorePrepareRetirement(backing)}
                            }
                            writeManifest(obj("type" to "remove","id" to id))
                        }
                        previous = manifest
                    }
                    candidate = null
                    held = preserved
                    manifest = withContext(Dispatchers.IO) { Native.sessionManifestRead(File(path,"session.json").absolutePath) }
                    ownsSource = true
                    if(current !== preserved) {
                        retained.remove(preserved)
                        if(current !in retained)retained.add(current)
                        owners.values.forEach {it.stamp = ""}
                    }
                    if(restoreAll(true)) {
                        if(held === preserved)retained.remove(preserved)
                        candidate = null
                    }
                } catch(e:CancellationException) {throw e}
                catch(e:Exception) {reportFailure(e,true);candidate = path}
                finally {
                    if(candidate != null&&!ownsSource) {
                        if(held !== current && held !== preserved)withContext(Dispatchers.IO) {held?.close()}
                        held = current; manifest = previous;owners.clear();owners.putAll(previousOwners)
                    }
                    restoring = false
                    working = false
                }
            }
            capture()?.join()
        }
    }
    fun dismiss() { candidate = null }
    fun close(after: () -> Unit) {
        closed = true; polling?.cancel()
        val accepted = scope.coroutineContext.job.children.toList()
        scope.launch {
            try {
                accepted.joinAll()
                if(started&&held!=null)try {
                    withTimeout(30_000) {while(!save(true)) {host.withNative {Native.sessionSettle(it,System.nanoTime())};delay(16)}}
                } catch(e:Exception) {reportFailure(e)}
            } finally {
                try {withContext(NonCancellable) {storage.withLock {withContext(Dispatchers.IO) {
                    try {stores.values.forEach(Native::sessionStoreFree)}
                    finally {try {held?.close()}finally {retained.forEach {it.close()}}}
                }}}} finally {try {after()}finally {scope.cancel()}}
            }
        }
    }
}
