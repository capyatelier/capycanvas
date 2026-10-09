package art.capycanvas

import android.view.KeyEvent
import androidx.compose.runtime.*
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.*
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import org.json.JSONObject

/** Native input and worker scheduling only. Membership, close/order policy,
 * labels, admission, retained history and backing ownership are shared Rust. */
internal class DrawingTabsController(private val host: CanvasHost) {
    var view by mutableStateOf(JSONObject()); private set
    var selector by mutableStateOf(false)
    /** The drawing tab with key focus. It closes on Delete, which canvas shortcuts then never see. */
    var focused: Long? = null
    fun takes(event:KeyEvent) = event.keyCode==KeyEvent.KEYCODE_FORWARD_DEL && focused.let { id -> rows.any { it.getLong("id")==id } }
    var switching by mutableStateOf(false); private set
    private fun transition(value:Boolean) {
        switching=value
        // Final close has retired the editor. Keep late UI disposal/input from
        // reaching it while workspace persistence and Activity finish drain.
        host.documentInputBlocked=value || selected==0L
    }
    var closingWindow = false; private set
    private var refreshing = false
    private val storage = Mutex()
    private val inspections = mutableMapOf<Long, Job>()
    val selected get() = view.optLong("selected", 1)
    val rows get() = view.array("tabs").objects()
    val blocked get() = switching || host.documents.working || host.documents.picker != null || host.recovery.working
    suspend fun query(request: JSONObject): String = host.withNative { Native.documentTabs(it, request.toString()) }
    fun refresh() {
        if (refreshing) return
        refreshing = true
        host.viewModelScope.launch {
            try { val next = JSONObject(query(obj("op" to "view"))); if (next.toString() != view.toString()) view = next }
            finally { refreshing = false }
        }
    }
    fun registerInspection(control: Long, job: Job) { inspections[control] = job }
    fun releaseInspection(control: Long) { inspections.remove(control) }
    private suspend fun drain() {
        host.proof.pauseAndDrain(); host.hdr.pause(); host.filterPreviewCache.pause()
        val pending=inspections.toMap(); pending.keys.forEach(Native::captureCancel); pending.values.forEach { it.join() }
    }
    private fun resume() { host.filterPreviewCache.resume(); host.proof.resume(); host.hdr.resume(); host.documentChanged(); refresh() }
    suspend fun waitReady(close: Boolean = false, settle: () -> Boolean = {false}) = withTimeout(30_000) {
        while (!JSONObject(query(obj("op" to "ready"))).optBoolean(if (close) "close" else "park")) {
            if(settle())host.withNative {Native.sessionSettle(it,System.nanoTime())} else host.documentChanged()
            delay(16)
        }
    }
    suspend fun trim() = storage.withLock {
        try {
            while (true) {
                val task=host.withNative { Native.documentSpillTask(it) }
                if (task==0L) break
                withContext(NonCancellable + Dispatchers.IO) { Native.documentSpillWork(task) }
            }
            query(obj("op" to "storage", "error" to null))
        } catch(e:CancellationException) { throw e }
        catch(e:Exception) {
            query(obj("op" to "storage", "error" to (e.message ?: "Drawing cache failed")))
            host.reportActionError(host.withNative {Native.sessionFailure(it,e.message.orEmpty(),false)})
        }
        refresh()
    }
    /** File candidate is fully prepared before completing its initiating request. */
    suspend fun beforeAdopt(task: Long) = beforeAdopt { host.withNative { Native.projectParkReady(it, task) } }
    suspend fun beforeAdopt(ready: suspend () -> Boolean) {
        check(!switching) { "Another drawing transition is active" }
        transition(true)
        try {
            drain()
            withTimeout(30_000) { while (!ready()) { host.documentChanged(); delay(16) } }
            host.recovery.capture()?.join()
        } catch(e:Exception) { afterAdopt(); throw e }
    }
    suspend fun beforeSessionAdopt(settle: () -> Boolean = {false}) {
        check(!switching) { "Another drawing transition is active" }
        transition(true)
        try { drain(); waitReady(settle=settle) } catch(e:Exception) { afterAdopt(); throw e }
    }
    suspend fun selectRestored(id: Long) {
        drain(); waitReady(); activate(id); trim(); refresh(); resume()
    }
    suspend fun afterAdopt() { try { trim(); host.recovery.ensureOwners() } catch(e:CancellationException){throw e} catch(e:Exception){host.reportActionError(host.withNative {Native.sessionFailure(it,e.message.orEmpty(),true)})} finally {transition(false);if(currentCoroutineContext().isActive)resume()} }
    private suspend fun activate(id: Long, close: Boolean = false) {
        var task=0L
        try {
            task=host.withNative { Native.documentSwitch(it,id,close) }
            if(task!=0L) {
                withContext(Dispatchers.IO) { Native.documentResumeWork(task) }
                host.withNative { Native.documentResume(it,task) }
                host.documentCanvasFailure(null)
            }
        } catch(e:CancellationException) { throw e }
        catch(e:Exception) { if(task!=0L)host.documentCanvasFailure(e.message ?: "Drawing renderer unavailable");throw e }
        finally { withContext(NonCancellable+Dispatchers.IO) { if(task!=0L) Native.documentResumeFree(task) } }
    }
    fun select(id:Long, close:Boolean=false) {
        if(blocked) return
        transition(true)
        host.viewModelScope.launch {
            try {
                val current=JSONObject(query(obj("op" to "view"))).getLong("selected")
                if(id==current) {
                    selector=false
                    if(close) {
                        drain();waitReady(true)
                        host.withNative{Native.dispatch(it,obj("type" to "invoke","command" to "close_document").toString())};host.documentChanged()
                    }
                    return@launch
                }
                check(JSONObject(query(obj("op" to "ready"))).optBoolean("available")) { "Finish the current operation before switching drawings" }
                drain(); waitReady(); host.recovery.capture()?.join()
                activate(id); selector=false
                trim(); refresh()
                if(close) { waitReady(true); host.withNative { Native.dispatch(it,obj("type" to "invoke","command" to "close_document").toString()) }; host.documentChanged() }
            } catch(e:CancellationException) { throw e }
            catch(e:Exception) { host.reportActionError(e.message ?: "Could not switch drawings") }
            finally { transition(false); if(currentCoroutineContext().isActive)resume() }
        }
    }
    fun closeSelected() { if(!blocked) { if(selected==0L)host.closeWorkspaceWindow() else select(selected,true) } }
    fun closeWindow() {
        if(blocked) return
        closingWindow=true; transition(true)
        host.viewModelScope.launch {
            var preserved=false
            try {
                drain(); waitReady()
                host.withNative { Native.sessionClose(it) }
                preserved=host.recovery.flush()
                if(preserved)host.closeWorkspaceWindow { cancelWindowClose() }
            } catch(e:CancellationException) { throw e }
            catch(e:Exception) { host.reportActionError(e.message ?: "Could not preserve drawing session") }
            finally {
                if(!preserved) {
                    query(obj("op" to "reset_close"));closingWindow=false;transition(false)
                    if(currentCoroutineContext().isActive)resume()
                }
            }
        }
    }
    private fun cancelWindowClose() {
        host.viewModelScope.launch {
            query(obj("op" to "reset_close"));closingWindow=false;transition(false);resume()
        }
    }
    fun cancelClose() { closingWindow=false }
    /** Approval is published by Compose, but the transaction belongs to the
     * window's ViewModel. Its own switching/epoch publications recompose the UI;
     * neither those publications nor Activity recreation may cancel retirement.
     */
    fun acceptClose() {
        if(switching || closingWindow || selected==0L) return
        transition(true)
        host.viewModelScope.launch {
            var prepared=0L;var activation=0L
            try {
                if(!JSONObject(query(obj("op" to "ready"))).optBoolean("approved"))return@launch
                val closingId=JSONObject(query(obj("op" to "view"))).getLong("selected")
                drain(); waitReady()
                prepared=host.withNative {Native.documentPrepareClose(it)}
                withContext(NonCancellable) {
                    host.recovery.remove(closingId)
                    activation=host.withNative {Native.documentCommitClose(it,prepared)};prepared=0L
                    host.recovery.closed(closingId)
                }
                withContext(Dispatchers.IO) {Native.documentResumeWork(activation)}
                host.withNative {Native.documentResume(it,activation)}
                host.documentCanvasFailure(null);trim()
                val next=JSONObject(query(obj("op" to "view"))); view=next
                if(next.array("tabs").length()==0) host.closeWorkspaceWindow()
            } catch(e:CancellationException) { throw e }
            catch(e:Exception) {
                if(activation!=0L)host.documentCanvasFailure(e.message ?: "Drawing renderer unavailable")
                closingWindow=false; host.reportActionError(e.message ?: "Could not close drawing")
            }
            finally {
                withContext(NonCancellable) {
                    if(prepared!=0L)host.withNative {Native.documentCancelPreparedClose(it,prepared)}
                    if(activation!=0L)withContext(Dispatchers.IO) {Native.documentResumeFree(activation)}
                }
                transition(false); if(currentCoroutineContext().isActive && selected!=0L)resume()
            }
        }
    }
    fun order(request:JSONObject) { host.viewModelScope.launch { reorder(request) } }
    suspend fun reorder(request:JSONObject) {
        if(blocked) return
        try { query(request);val next=JSONObject(query(obj("op" to "view")));if(next.toString()!=view.toString())view=next;host.documentChanged() }
        catch(e:CancellationException){throw e}
        catch(e:Exception){host.reportActionError(e.message ?: "Could not reorder drawings")}
    }
    fun adjacent(forward:Boolean) {
        if(!blocked) host.viewModelScope.launch {
            val id=query(obj("op" to "adjacent","forward" to forward)).toLongOrNull()
            if(id!=null)select(id)
        }
    }
    fun key(event:KeyEvent):Boolean {
        if(event.action!=KeyEvent.ACTION_DOWN) return false
        if(event.isCtrlPressed&&event.isAltPressed) when(event.keyCode) {
            KeyEvent.KEYCODE_D->{selector=true;return true}
            KeyEvent.KEYCODE_W->{closeSelected();return true}
        }
        return false
    }
}
