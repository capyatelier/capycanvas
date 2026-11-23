package art.capycanvas

import androidx.compose.ui.test.junit4.createAndroidComposeRule
import kotlinx.coroutines.runBlocking
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test

class AndroidStampTransportTest {
    @get:Rule(order=0) val device = CapyDeviceRule()
    @get:Rule(order=1) val compose = createAndroidComposeRule<MainActivity>()

    @Test fun toolbarAndBubbleStampBytesMatchTheCanonicalMaskWithoutJsonAlpha() {
        val host=compose.activity.host
        host.awaitReady(compose=compose)
        val brush=host.catalog.getJSONArray("brush_categories").objects().flatMap { it.getJSONArray("brushes").objects() }
            .first { it.getString("label")=="G-Pen" }.getLong("id")
        host.drain(obj("type" to "select_brush","id" to brush))
        host.drain(obj("type" to "set_brush_size","value" to 40))
        host.awaitReady(compose=compose)
        val snapshot=host.snapshot!!
        val context=snapshot.array("phone_sliders").objects().firstOrNull()?.getJSONObject("context")
            ?: snapshot.array("panels").objects().flatMap { it.array("tiles").objects() }
                .firstNotNullOf { it.objectOrNull("component")?.objectOrNull("context") }
        val method=Native::class.java.methods.firstOrNull { it.name=="toolbarStamp" }
        fun raw(context:JSONObject?,known:Long=0):Pair<JSONObject,ByteArray>? = runBlocking { host.withNative { handle ->
            if(method==null) {
                val query=if(context==null)obj("type" to "bubble_stamp") else obj("type" to "toolbar_stamp","context" to context)
                if(known!=0L)query.put("known_request",known)
                (org.json.JSONTokener(Native.query(handle,query.toString())).nextValue() as? JSONObject)?.let { it to byteArrayOf() }
            } else {
                val reply=method.invoke(null,handle,context?.toString().orEmpty(),known) as? Array<*>
                reply?.let { JSONObject(it[0] as String) to (it[1] as ByteArray) }
            }
        } }
        fun ready(context:JSONObject?):Pair<JSONObject,ByteArray> {
            var result:Pair<JSONObject,ByteArray>?=null
            compose.waitUntil(30_000) {
                result=raw(context)
                host.canvasChanged()
                result?.let { !it.first.optBoolean("pending") && (it.second.isNotEmpty() || it.first.has("alpha")) }==true
            }
            return checkNotNull(result)
        }
        val canonical=run {
            var result:JSONObject?=null
            compose.waitUntil(30_000) {
                result=runBlocking { host.withNative { handle ->
                    org.json.JSONTokener(Native.query(handle,obj("type" to "toolbar_stamp","context" to context).toString())).nextValue() as? JSONObject
                } }
                host.canvasChanged()
                result?.optJSONArray("alpha")?.length()?.let { it>0 }==true && result?.optBoolean("pending")==false
            }
            checkNotNull(result)
        }
        val alpha=canonical.getJSONArray("alpha")
        val bytes=ByteArray(alpha.length()) { alpha.getInt(it).toByte() }
        assertTrue("The canonical G-Pen mask contains coverage",bytes.any { it.toInt()!=0 })
        for(owner in listOf(context,null)) {
            val (metadata,mask)=ready(owner)
            assertFalse("Stamp metadata must not carry a JSON alpha array",metadata.has("alpha"))
            assertArrayEquals("The typed mask equals the canonical JSON-query mask",bytes,mask)
            assertEquals(canonical.getInt("width"),metadata.getInt("width"));assertEquals(canonical.getInt("height"),metadata.getInt("height"))
            assertEquals(canonical.getDouble("extent"),metadata.getDouble("extent"),0.0)
            val (known,empty)=checkNotNull(raw(owner,metadata.getLong("request_id")))
            assertEquals(metadata.getLong("request_id"),known.getLong("request_id"))
            assertFalse(known.getBoolean("pending"));assertFalse(known.has("alpha"));assertEquals(0,empty.size)
        }
        val stale=context.copy().put("generation",context.getLong("generation")+1)
        runBlocking { host.withNative { handle ->
            try { method!!.invoke(null,handle,stale.toString(),0L);fail("Stale contexts must be refused") }
            catch(error:java.lang.reflect.InvocationTargetException) { assertTrue(error.cause is IllegalStateException) }
            val retired=Native.captureControl()
            Native.captureFree(retired)
            for(invalid in listOf(0L,Long.MAX_VALUE,retired)) {
                try { method!!.invoke(null,invalid,context.toString(),0L);fail("Invalid handles must be refused") }
                catch(error:java.lang.reflect.InvocationTargetException) { assertTrue(error.cause is IllegalStateException) }
            }
        } }
    }
}
