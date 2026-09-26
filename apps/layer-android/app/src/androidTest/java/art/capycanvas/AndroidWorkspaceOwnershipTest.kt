package art.capycanvas

import android.os.SystemClock
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import java.io.File

/** Exercises the Android kernel-lock path using independent native sessions. */
class AndroidWorkspaceOwnershipTest {
    @get:Rule val device = CapyDeviceRule()
    private fun request(handle: Long, input: JSONObject) = JSONObject(Native.workspace(handle, input.toString())).getJSONObject("view")
    private fun settled(handle: Long): JSONObject {
        val deadline = SystemClock.uptimeMillis() + 30_000
        do {
            val view = request(handle, obj("type" to "tick"))
            if (!view.optBoolean("busy") && (view.optBoolean("ready") || !view.isNull("error"))) return view
            SystemClock.sleep(10)
        } while (SystemClock.uptimeMillis() < deadline)
        error("Native workspace did not settle")
    }
    @Test fun liveOwnersRemainExclusiveAndClosedSessionsReleaseTheirLocks() {
        val directory = File(device.root, "ownership")
        var first = Native.create(false)
        val second = Native.create(false)
        try {
            fun start(handle: Long): JSONObject {
                request(handle, obj("type" to "start", "directory" to directory.absolutePath))
                return settled(handle).also { assertTrue(it.toString(), it.optBoolean("ready") && it.isNull("error")) }
            }
            val firstId = start(first).getString("id")
            val secondId = start(second).getString("id")
            assertNotEquals("A live owner keeps its workspace", firstId, secondId)
            request(second, obj("type" to "switch", "id" to firstId))
            val occupied = settled(second)
            assertEquals(secondId, occupied.getString("id"))
            assertEquals("Switch requests the existing owner instead of taking its lock", firstId, occupied.getString("focus_window"))
            Native.destroy(first); first = 0
            request(second, obj("type" to "switch", "id" to firstId))
            val released = settled(second)
            assertEquals(firstId, released.getString("id"))
            assertTrue(released.toString(), released.isNull("error"))
        } finally {
            if (first != 0L) Native.destroy(first)
            Native.destroy(second)
        }
    }
}
