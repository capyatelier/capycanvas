package art.capycanvas

import android.net.Uri
import android.os.ParcelFileDescriptor
import androidx.test.core.app.ActivityScenario
import androidx.lifecycle.Lifecycle
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import java.io.File

class AndroidSessionRestartTest {
    @get:Rule val device = CapyDeviceRule(nativeFileJobs = true)
    @Test fun processRestartRetainsSavedAndUnsavedHistory() {
        val arguments = InstrumentationRegistry.getArguments()
        val phase = arguments.getString("restartPhase")
        assumeTrue(phase in listOf("prepare","verify"))
        require(arguments.getString("restartFixture") != null)
        wakeDevice()
        val scenario = launchCapy(120_000)
        val host = scenario.activity().host
        fun <T> native(block:(Long)->T):T = runBlocking {host.withNative(block)}
        fun state(): JSONObject {
            var result: JSONObject? = null
            instrumentation.runOnMainSync {result = host.snapshot?.getJSONObject("state")?.copy()}
            return checkNotNull(result)
        }
        fun tabs() = native {JSONObject(Native.documentTabs(it,obj("op" to "view").toString()))}
        fun flush() = runBlocking {withContext(Dispatchers.Main) {host.recovery.flush()}}
        try {
            if(phase == "prepare") {
                val background = arguments.getString("restartExit") == "background"
                arguments.getString("theme","light")!!.let {host.drain(obj("type" to "set_theme","theme" to it))}
                host.drain(obj("type" to "invoke","command" to "add_layer"))
                val first = tabs().getLong("selected")
                val file = File(device.root,"saved.capy")
                val location = obj("uri" to Uri.fromFile(file).toString(),"name" to file.name)
                val task = native {h ->val(id,owner)=documentRequest(h,"save_document_as");
                    Native.projectTask(h,id,location.toString(),owner.getLong("epoch"),owner.getLong("revision")) to id}
                try {
                    Native.projectWork(task.first,ParcelFileDescriptor.open(file,ParcelFileDescriptor.MODE_CREATE or ParcelFileDescriptor.MODE_TRUNCATE or ParcelFileDescriptor.MODE_READ_WRITE).detachFd(),0,0)
                    val fingerprint = Native.sessionFingerprint(ParcelFileDescriptor.open(file,ParcelFileDescriptor.MODE_READ_ONLY).detachFd())
                    native {Native.sessionCompleteSave(it,task.second,location.toString(),fingerprint)}
                } finally {Native.projectFree(task.first)}
                host.newDocument(640,480)
                host.drain(obj("type" to "invoke","command" to "add_layer"));host.drain(obj("type" to "invoke","command" to "add_layer"))
                host.drain(obj("type" to "invoke","command" to "zoom_in"))
                assertTrue(flush())
                File(device.root,"expected.json").writeText(obj("first" to first,"second" to tabs().getLong("selected"),
                    "layers" to state().array("layers").length(),"zoom" to state().getJSONObject("camera").getDouble("zoom"),"location" to location,"recovered" to !background).toString())
                val manifest = device.recovery.walkTopDown().first {it.name == "session.json"}
                if(background) {
                    repeat(2) {
                        scenario.moveToState(Lifecycle.State.CREATED)
                        assertTrue(flush())
                        val before = Native.sessionManifestRead(manifest.absolutePath)
                        assertTrue(JSONObject(before).getBoolean("clean_exit"))
                        android.os.SystemClock.sleep(4500)
                        assertEquals("The background poll preserves a clean checkpoint",before,Native.sessionManifestRead(manifest.absolutePath))
                        if(it == 0) {
                            scenario.moveToState(Lifecycle.State.RESUMED)
                            host.awaitMain("foreground marks the live session",30_000) {
                                !JSONObject(Native.sessionManifestRead(manifest.absolutePath)).getBoolean("clean_exit")
                            }
                        }
                    }
                } else {
                    val original = Native.sessionManifestRead(manifest.absolutePath)
                    val current = JSONObject(original)
                    Native.sessionManifestWrite(manifest.absolutePath,Native.sessionManifestUpdate(original,obj("type" to "stage","drawings" to current.getJSONArray("drawings"),"active" to current.getLong("active")).toString()))
                }
                println("RESTART_PREPARED ${device.root.name}")
                android.os.Process.killProcess(android.os.Process.myPid())
            } else {
                val expected = JSONObject(File(device.root,"expected.json").readText())
                host.awaitMain("all private drawings restored",120_000) {!host.recovery.working&&host.recovery.ready}
                assertNull(host.recovery.candidate)
                assertEquals(listOf(expected.getLong("first"),expected.getLong("second")),tabs().array("tabs").objects().map {it.getLong("id")})
                assertEquals(expected.getLong("second"),tabs().getLong("selected"))
                assertEquals(expected.getDouble("zoom"),state().getJSONObject("camera").getDouble("zoom"),1e-9)
                assertTrue(state().getJSONObject("document_file").getBoolean("modified"))
                assertEquals(expected.getBoolean("recovered"),state().getJSONObject("document_file").getBoolean("recovered"))
                assertEquals(expected.getInt("layers"),state().array("layers").length())
                host.drain(obj("type" to "invoke","command" to "undo"));assertEquals(expected.getInt("layers")-1,state().array("layers").length())
                host.drain(obj("type" to "invoke","command" to "redo"));assertEquals(expected.getInt("layers"),state().array("layers").length())
                instrumentation.runOnMainSync {host.drawingTabs.select(expected.getLong("first"))}
                host.awaitMain("saved drawing selected",120_000) {!host.drawingTabs.switching&&host.drawingTabs.selected == expected.getLong("first")}
                assertFalse(state().getJSONObject("document_file").getBoolean("modified"))
                assertEquals(expected.getBoolean("recovered"),state().getJSONObject("document_file").getBoolean("recovered"))
                assertEquals(jsonValue(expected.getJSONObject("location")),jsonValue(state().getJSONObject("document_file").getJSONObject("location")))
                host.drain(obj("type" to "invoke","command" to "undo"));assertTrue(state().getJSONObject("document_file").getBoolean("modified"))
                host.drain(obj("type" to "invoke","command" to "redo"));assertFalse(state().getJSONObject("document_file").getBoolean("modified"))
            }
        } finally {scenario.close()}
    }
}
