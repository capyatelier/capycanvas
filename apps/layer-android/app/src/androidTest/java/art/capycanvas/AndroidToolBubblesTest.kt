package art.capycanvas

import android.os.SystemClock
import android.view.MotionEvent
import android.view.ViewConfiguration
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.platform.ViewRootForTest
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.test.core.app.ActivityScenario
import org.json.JSONArray
import org.json.JSONObject
import org.junit.After
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.Assert.*
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlin.math.cos
import kotlin.math.sin

class AndroidToolBubblesTest {
    @get:Rule val device = CapyDeviceRule()
    private lateinit var scenario: ActivityScenario<MainActivity>
    private lateinit var host: CanvasHost
    private lateinit var fixture: JSONObject
    private var density = 1f
    private var root: ViewRootForTest? = null
    private var point = Offset.Zero
    private var downAt = 0L
    private var tool = MotionEvent.TOOL_TYPE_FINGER
    private var pressed = false
    private val contacts = JSONArray()
    private var case = "setup"
    private fun shot(name: String, inspect: ((android.graphics.Bitmap) -> Unit)? = null) = screenshot("validation/tool-bubbles/$case-$name.png", inspect)
    private fun state() = host.snapshot!!.getJSONObject("state")
    private fun action(value: JSONObject) { host.drain(value); instrumentation.runOnMainSync { assertNull(host.failure); assertNull(host.actionError) } }
    private fun invoke(command: String) = action(obj("type" to "invoke", "command" to command))
    private fun query(value: JSONObject): Any? {
        val done = CountDownLatch(1)
        var result: Any? = null
        instrumentation.runOnMainSync { host.query(value) { result = it; done.countDown() } }
        assertTrue("Query $value completes", done.await(15, TimeUnit.SECONDS))
        return result
    }
    private fun packets() = (query(obj("type" to "bubbles")) as JSONArray).objects()
    private fun view(id: Long) = packets().single { it.getJSONObject("view").getLong("id") == id }.getJSONObject("view")
    private fun placements() = (query(obj("type" to "bubble_motion", "now_ns" to System.nanoTime())) as JSONObject).getJSONArray("placements").objects()
    private fun center(id: Long): Offset {
        val p = placements().single { it.getLong("id") == id }.getJSONArray("center")
        return workspace().topLeft + Offset(p.getDouble(0).toFloat(), p.getDouble(1).toFloat()) * density
    }
    private fun faceCenter(id: Long): Offset {
        val color = packets().single { it.getJSONObject("view").getLong("id") == id }.objectOrNull("color") ?: return center(id)
        val bounds = color.getJSONObject("bounds")
        return center(id) + Offset(bounds.number("x") + bounds.number("width") / 2, bounds.number("y") + bounds.number("height") / 2) * density
    }
    private fun bounds(tag: String): Rect {
        var result: Rect? = null
        instrumentation.runOnMainSync {
            result = if (tag.startsWith("bubble-properties-")) findTag("bubble-card-" + tag.removePrefix("bubble-properties-"))?.second?.find(hasTag("layer-properties-disclosure"))?.boundsInRoot
                else findTag(tag)?.second?.boundsInRoot
        }
        return checkNotNull(result) { "Missing $tag" }
    }
    private fun workspace() = bounds("workspace")
    private fun waitFor(label: String, details: () -> String = { "" }, condition: () -> Boolean) {
        val deadline = SystemClock.uptimeMillis() + 15000
        while (SystemClock.uptimeMillis() < deadline) {
            instrumentation.runOnMainSync { assertNull(host.failure); assertNull(host.actionError) }
            if (condition()) return
            SystemClock.sleep(32)
        }
        screenshot("validation/tool-bubbles/failure.png")
        java.io.File(instrumentation.targetContext.getExternalFilesDir(null), "validation/tool-bubbles/contact-failure.json").writeText(obj("stage" to label, "contacts" to contacts, "published" to host.snapshot?.optJSONArray("bubbles"), "core" to JSONArray(packets())).toString(2))
        fail("$label; ${details()}; bubbles=${packets()}; motion=${query(obj("type" to "bubble_motion", "now_ns" to System.nanoTime()))}")
    }
    private fun stable(id: Long): Offset {
        var before = center(id)
        repeat(100) {
            SystemClock.sleep(50)
            val after = center(id)
            if ((after - before).getDistance() < .1f) {
                SystemClock.sleep(120)
                if ((center(id) - after).getDistance() < .1f) return after
            }
            before = after
        }
        error("Bubble $id did not settle")
    }
    private fun event(action: Int, next: Offset = point) {
        point = next
        if (action == MotionEvent.ACTION_DOWN) {
            downAt = SystemClock.uptimeMillis()
            if (root == null) instrumentation.runOnMainSync { root = checkNotNull(findTag("workspace")).first }
            pressed = true
        }
        val event = motion(tool, action, next, downAt)
        try { instrumentation.runOnMainSync { val handled = checkNotNull(root).view.dispatchTouchEvent(event); contacts.put(obj("action" to action, "tool" to tool, "x" to next.x, "y" to next.y, "handled" to handled, "time" to event.eventTime)) } }
        finally { event.recycle() }
        if (action == MotionEvent.ACTION_UP || action == MotionEvent.ACTION_CANCEL) pressed = false
        SystemClock.sleep(40)
    }
    private fun tap(at: Offset) { event(MotionEvent.ACTION_DOWN, at); event(MotionEvent.ACTION_UP); host.drain(); instrumentation.runOnMainSync { assertNull(host.failure); assertNull("Native tap at $at must not dispatch a stale action", host.actionError) } }
    private fun doubleTap(first: Offset, second: Offset) {
        val start = contacts.length()
        event(MotionEvent.ACTION_DOWN, first); event(MotionEvent.ACTION_UP)
        event(MotionEvent.ACTION_DOWN, second); event(MotionEvent.ACTION_UP)
        val elapsed = contacts.getJSONObject(start + 2).getLong("time") - contacts.getJSONObject(start).getLong("time")
        assertTrue("Deliberate double contact stays within the platform timeout: $elapsed", elapsed <= ViewConfiguration.getDoubleTapTimeout())
        val slop = maxOf(8 * density, ViewConfiguration.get(instrumentation.targetContext).scaledDoubleTapSlop.toFloat())
        assertTrue("Deliberate double contact stays within the platform distance", (second - first).getDistance() <= slop)
        host.drain()
    }
    private fun stroke(first: Offset, second: Offset, third: Offset? = null, fourth: Offset? = null) {
        val points = listOfNotNull(first, second, third, fourth)
        event(MotionEvent.ACTION_DOWN, points.first())
        points.drop(1).forEach { event(MotionEvent.ACTION_MOVE, it) }
        event(MotionEvent.ACTION_UP)
        host.drain()
    }
    private fun take(header: Int, target: Offset): Long {
        val before = packets().map { it.getJSONObject("view").getLong("id") }.toSet()
        val from = bounds("header-item-$header").center
        stroke(from, from + Offset(0f, 8 * density), from + Offset(0f, 80 * density), target)
        waitFor("Header source $header tears out") { packets().any { it.getJSONObject("view").getLong("id") !in before } }
        return packets().single { it.getJSONObject("view").getLong("id") !in before }.getJSONObject("view").getLong("id").also { id ->
            stable(id)
            assertTrue("A released source has no inherited ring dab: ${view(id)}", view(id).isNull("stamp"))
            waitFor("Native source presentation reaches the shared placement", { "view=${view(id)}; shared=${center(id)}; native=${findTag(if (view(id).isNull("card")) "tool-bubble-$id" else "bubble-card-$id")?.second?.boundsInRoot}" }) {
                val tag = if (view(id).isNull("card")) "tool-bubble-$id" else "bubble-card-$id"
                var native: Offset? = null
                instrumentation.runOnMainSync { native = findTag(tag)?.second?.boundsInRoot?.center }
                native?.let { (it - faceCenter(id)).getDistance() < 2f } == true
            }
        }
    }
    private fun putBack(id: Long) {
        val v = view(id)
        val c = stable(id)
        val from = if (v.getString("kind") == "color") c - Offset(v.number("diameter") * density / 2 - 4 * density, 0f) else c
        val tag = if (v.isNull("card")) "tool-bubble-$id" else "bubble-card-$id"
        val native = bounds(tag)
        val local = (from - c) / density
        val before = obj("view" to v, "nativeBounds" to native.toString(), "center" to c.toString(), "from" to from.toString(), "local" to local.toString(), "hit" to query(obj("type" to "bubble_hit", "id" to id, "x" to local.x, "y" to local.y)), "motion" to query(obj("type" to "bubble_motion", "now_ns" to System.nanoTime())))
        shot("return-$id-before")
        assertTrue("Return starts at the actual native center: native=${native.center}, shared=${faceCenter(id)}", (native.center - faceCenter(id)).getDistance() < 2f)
        stroke(from, from - Offset(0f, 30 * density), Offset(from.x, bounds("title-bar").center.y))
        java.io.File(instrumentation.targetContext.getExternalFilesDir(null), "validation/tool-bubbles/return-diagnostic.json").writeText(obj("before" to before, "afterBubbles" to JSONArray(packets()), "afterMotion" to query(obj("type" to "bubble_motion", "now_ns" to System.nanoTime())), "contacts" to contacts).toString(2))
        waitFor("Returning $id removes it") { packets().none { it.getJSONObject("view").getLong("id") == id } }
    }
    private fun ringPoint(id: Long, ring: JSONObject, fraction: Float): Offset {
        val arc = ring.getJSONObject("arc")
        val angle = Math.toRadians((arc.number("start") + arc.number("sweep") * fraction).toDouble())
        return center(id) + Offset(cos(angle).toFloat(), sin(angle).toFloat()) * arc.number("radius") * density
    }
    private fun setting(id: String) = state().getJSONArray("tool_settings").objects().single { it.getString("id") == id }.get("value").toString()
    private fun menu(id: Long) = query(obj("type" to "context", "target" to obj("kind" to "bubble", "id" to id))) as JSONObject
    private fun menuItems(id: Long) = menu(id).getJSONArray("sections").values().flatMap { (it as JSONArray).objects() }
    private fun tapMenu(label: String) {
        var target: Pair<ViewRootForTest, SemanticsNode>? = null
        waitFor("Menu row $label") {
            instrumentation.runOnMainSync {
                fun text(n: SemanticsNode): Boolean = n.config.getOrNull(SemanticsProperties.Text)?.any { it.text == label } == true || n.children.any(::text)
                target = findNode({ it.config.getOrNull(SemanticsActions.OnClick) != null && text(it) })
            }
            target != null
        }
        root = target!!.first
        tap(target!!.second.boundsInRoot.center)
        root = null
    }
    private fun hold(id: Long) {
        event(MotionEvent.ACTION_DOWN, stable(id))
        SystemClock.sleep(ViewConfiguration.getLongPressTimeout().toLong() * (if (view(id).getString("kind") == "color") 2 else 1) + 250)
        event(MotionEvent.ACTION_UP)
        waitFor("Held bubble opens native menu") { findTag("workspace-menu") != null }
    }
    private fun escape() { val hadMenu = findTag("workspace-menu") != null; pressKey(android.view.KeyEvent.KEYCODE_ESCAPE); host.drain(); root = null; if (hadMenu) waitFor("Escape dismisses native bubble menu") { findTag("workspace-menu") == null } }
    private fun twoTouches(id: Long, firstTool: Int = MotionEvent.TOOL_TYPE_FINGER) {
        val c = stable(id)
        val oldSize = view(id).number("diameter")
        val start = SystemClock.uptimeMillis()
        instrumentation.runOnMainSync { root = checkNotNull(findTag("workspace")).first }
        fun send(action: Int, points: List<Offset>) {
            val e = MotionEvent.obtain(start, SystemClock.uptimeMillis(), action, points.size,
                points.indices.map { MotionEvent.PointerProperties().apply { this.id = it; toolType = if (it == 0) firstTool else MotionEvent.TOOL_TYPE_FINGER } }.toTypedArray(),
                points.map { MotionEvent.PointerCoords().apply { x = it.x; y = it.y; pressure = .7f } }.toTypedArray(),
                0, 0, 1f, 1f, 0, 0, android.view.InputDevice.SOURCE_TOUCHSCREEN, 0)
            try { instrumentation.runOnMainSync { root!!.view.dispatchTouchEvent(e) } } finally { e.recycle() }
            SystemClock.sleep(50)
        }
        send(MotionEvent.ACTION_DOWN, listOf(c - Offset(20 * density, 0f)))
        send(MotionEvent.ACTION_POINTER_DOWN or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), listOf(c - Offset(20 * density, 0f), c + Offset(20 * density, 0f)))
        val moved = listOf(c + Offset(-55 * density, 50 * density), c + Offset(25 * density, 50 * density))
        send(MotionEvent.ACTION_MOVE, moved)
        send(MotionEvent.ACTION_POINTER_UP or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), moved)
        send(MotionEvent.ACTION_UP, listOf(moved[0]))
        root = null
        if (firstTool == MotionEvent.TOOL_TYPE_STYLUS) {
            assertEquals("Pen plus finger does not resize the wheel", oldSize, view(id).number("diameter"), .01f)
            assertTrue("Pen plus finger does not pair into a bubble drag", (stable(id) - c).getDistance() < 2f)
        } else {
            waitFor("Pinch resizes wheel") { view(id).number("diameter") > oldSize }
            assertTrue("Two fingers move the wheel", (stable(id) - c).getDistance() > 25 * density)
        }
    }
    private fun wheelColor() = query(obj("type" to "bubble_wheel_color")).toString()
    private fun secondFingerRestoresRing(id: Long) {
        val savedTool = tool; tool = MotionEvent.TOOL_TYPE_FINGER
        SystemClock.sleep(ViewConfiguration.getDoubleTapTimeout().toLong() + 1)
        val ring = view(id).getJSONArray("rings").objects().first()
        val key = ring.getString("setting"); val before = setting(key)
        val first = ringPoint(id, ring, .2f); val second = center(id)
        val started = SystemClock.uptimeMillis()
        instrumentation.runOnMainSync { root = checkNotNull(findTag("workspace")).first }
        fun send(action: Int, points: List<Offset>) {
            val e = MotionEvent.obtain(started, SystemClock.uptimeMillis(), action, points.size,
                points.indices.map { MotionEvent.PointerProperties().apply { this.id = it; toolType = MotionEvent.TOOL_TYPE_FINGER } }.toTypedArray(),
                points.map { MotionEvent.PointerCoords().apply { x = it.x; y = it.y; pressure = .7f } }.toTypedArray(),
                0, 0, 1f, 1f, 0, 0, android.view.InputDevice.SOURCE_TOUCHSCREEN, 0)
            try { instrumentation.runOnMainSync { root!!.view.dispatchTouchEvent(e) } } finally { e.recycle() }
            SystemClock.sleep(50)
        }
        send(MotionEvent.ACTION_DOWN, listOf(first))
        send(MotionEvent.ACTION_MOVE, listOf(ringPoint(id, ring, .8f)))
        host.drain()
        assertNotEquals("First ring contact changes its value before takeover", before, setting(key))
        send(MotionEvent.ACTION_POINTER_DOWN or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), listOf(ringPoint(id, ring, .8f), second))
        host.drain()
        assertEquals("Second finger restores the first ring contact", before, setting(key))
        send(MotionEvent.ACTION_POINTER_UP or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), listOf(ringPoint(id, ring, .8f), second))
        send(MotionEvent.ACTION_UP, listOf(ringPoint(id, ring, .8f)))
        root = null; tool = savedTool
    }
    private fun secondFingerTakesLayerRow(id: Long) {
        fun rows() = state().getJSONArray("layers").objects().map { layer ->
            listOf("id", "parent", "visible", "locked", "mask_selected", "clipped", "reference").associateWith { key -> layer.opt(key)?.toString() }
        }
        val before = rows(); val oldCenter = stable(id)
        val layerId = state().getJSONArray("layers").getJSONObject(0).getLong("id")
        var row: Rect? = null
        instrumentation.runOnMainSync { row = findTag("bubble-card-$id")?.second?.find(hasTag("layer-row-$layerId"))?.boundsInRoot }
        val first = checkNotNull(row).center; val second = first + Offset(30 * density, 0f)
        val started = SystemClock.uptimeMillis()
        instrumentation.runOnMainSync { root = checkNotNull(findTag("workspace")).first }
        fun send(action: Int, points: List<Offset>) {
            val e = MotionEvent.obtain(started, SystemClock.uptimeMillis(), action, points.size,
                points.indices.map { MotionEvent.PointerProperties().apply { this.id = it; toolType = MotionEvent.TOOL_TYPE_FINGER } }.toTypedArray(),
                points.map { MotionEvent.PointerCoords().apply { x = it.x; y = it.y; pressure = .7f } }.toTypedArray(),
                0, 0, 1f, 1f, 0, 0, android.view.InputDevice.SOURCE_TOUCHSCREEN, 0)
            try { instrumentation.runOnMainSync { root!!.view.dispatchTouchEvent(e) } } finally { e.recycle() }
            SystemClock.sleep(50)
        }
        send(MotionEvent.ACTION_DOWN, listOf(first))
        send(MotionEvent.ACTION_POINTER_DOWN or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), listOf(first, second))
        val delta = Offset(60 * density, 30 * density)
        send(MotionEvent.ACTION_MOVE, listOf(first + delta, second + delta))
        send(MotionEvent.ACTION_POINTER_UP or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), listOf(first + delta, second + delta))
        send(MotionEvent.ACTION_UP, listOf(first + delta)); root = null; host.drain()
        assertEquals("Two-finger takeover cannot swipe or reorder the captured layer row", before, rows())
        assertTrue("Two fingers take the whole Layers card", (stable(id) - oldCenter).getDistance() > 20 * density)
    }
    private fun rapidHeaderTake(target: Offset) {
        val before = packets().map { it.getJSONObject("view").getLong("id") }.toSet()
        val from = bounds("header-item-2").center
        instrumentation.runOnMainSync {
            val owner = checkNotNull(findTag("workspace")).first
            val started = SystemClock.uptimeMillis()
            for ((phase, point) in listOf(MotionEvent.ACTION_DOWN to from, MotionEvent.ACTION_MOVE to target, MotionEvent.ACTION_UP to target)) {
                val event = motion(tool, phase, point, started)
                try { owner.view.dispatchTouchEvent(event) } finally { event.recycle() }
            }
        }
        host.drain()
        waitFor("Rapid header down/move/up does not wait for a query reply") { packets().any { it.getJSONObject("view").getLong("id") !in before } }
        val id = packets().single { it.getJSONObject("view").getLong("id") !in before }.getJSONObject("view").getLong("id")
        waitFor("Rapid pickup is presented at its shared center") { findTag("tool-bubble-$id")?.second?.boundsInRoot?.center?.let { (it - center(id)).getDistance() < 2f } == true }
        putBack(id)
    }
    private fun replacingCandidate(target: Offset) {
        val from = bounds("header-item-2").center
        event(MotionEvent.ACTION_DOWN, from)
        val replacement = JSONObject(fixture.toString())
        replacement.getJSONObject("layout").getJSONObject("header").getJSONArray("zones").getJSONArray(1).getJSONObject(0)
            .put("item", obj("kind" to "tool", "control" to obj("kind" to "color")))
        action(obj("type" to "restore_workspace", "workspace" to replacement))
        event(MotionEvent.ACTION_MOVE, target); event(MotionEvent.ACTION_UP); host.drain()
        assertTrue("Replacing a source candidate under the same ID cancels pickup", packets().isEmpty())
        action(obj("type" to "restore_workspace", "workspace" to JSONObject(fixture.toString())))
    }
    private fun heldTile(target: Offset) {
        val panel = fixture.getJSONObject("layout").getJSONArray("panels").objects().first { p ->
            p.objectOrNull("content")?.optJSONArray("tiles")?.objects()?.any { it.getJSONObject("control").optString("command") == "undo" } == true
        }
        val panelId = panel.getString("id")
        val tile = panel.getJSONObject("content").getJSONArray("tiles").objects().first { it.getJSONObject("control").optString("command") == "undo" }
        action(obj("type" to "customize", "action" to obj("type" to "set_panel_visible", "panel" to panelId, "visible" to true)))
        val from = bounds("tile-$panelId-${tile.getInt("id")}").center
        event(MotionEvent.ACTION_DOWN, from)
        SystemClock.sleep(ViewConfiguration.getLongPressTimeout().toLong() + 250)
        event(MotionEvent.ACTION_MOVE, from + Offset(40 * density, 0f)); event(MotionEvent.ACTION_MOVE, target); event(MotionEvent.ACTION_UP)
        waitFor("Held toolbar tile becomes a history bubble") { packets().any { it.getJSONObject("view").getString("kind") == "history" } }
        val id = packets().single { it.getJSONObject("view").getString("kind") == "history" }.getJSONObject("view").getLong("id")
        invoke("undo_workspace"); waitFor("Held tile pickup is one workspace undo") { packets().isEmpty() }
        invoke("redo_workspace"); waitFor("Held tile pickup redo restores its ID") { packets().any { it.getJSONObject("view").getLong("id") == id } }
        putBack(id)
    }
    private fun paletteControls(id: Long) {
        val palettes = view(id).getJSONArray("beads").objects().first { it.getJSONObject("placement").getJSONObject("bead").getString("kind") == "palettes" }
        val p = palettes.getJSONObject("placement").getJSONArray("center")
        val sharedCenter = center(id); val nativeCenter = bounds("tool-bubble-$id").center
        assertTrue("Color center after bead expansion: shared=$sharedCenter native=$nativeCenter", (sharedCenter - nativeCenter).getDistance() < 2f)
        val local = Offset(p.getDouble(0).toFloat(), p.getDouble(1).toFloat())
        val hit = query(obj("type" to "bubble_hit", "id" to id, "x" to local.x, "y" to local.y))
        assertEquals("Palette center has shared bead hit", "bead", hit)
        shot("wheel-beads-before-pill")
        tap(sharedCenter + local * density)
        val diagnostic = obj("center" to JSONArray(listOf(sharedCenter.x, sharedCenter.y)), "local" to p, "hit" to hit, "before" to palettes, "after" to view(id), "contacts" to contacts)
        java.io.File(instrumentation.targetContext.getExternalFilesDir(null), "validation/tool-bubbles/palette-diagnostic.json").writeText(diagnostic.toString(2))
        waitFor("Palette bead opens pill") { !view(id).isNull("pill") }
        val area = workspace()
        val edgeStart = (center(id) - area.topLeft) / density
        val edge = Offset(area.width / density - 4, area.height / density - 4)
        for ((phase, position) in listOf("down" to edgeStart, "move" to edge, "up" to edge)) action(obj("type" to "bubble", "action" to obj("type" to "drag", "grab" to obj("kind" to "bubble", "id" to id), "phase" to phase, "position" to JSONArray(listOf(position.x, position.y)), "viewport" to JSONArray(listOf(area.width / density, area.height / density)))))
        stable(id)
        assertTrue("Dragging closes the palette pill", view(id).isNull("pill"))
        val edgeBead = view(id).getJSONArray("beads").objects().single { it.getJSONObject("placement").getJSONObject("bead").getString("kind") == "palettes" }.getJSONObject("placement").getJSONArray("center")
        tap(center(id) + Offset(edgeBead.getDouble(0).toFloat(), edgeBead.getDouble(1).toFloat()) * density)
        waitFor("Palette reopens at the edge") { !view(id).isNull("pill") }
        stable(id); SystemClock.sleep(250)
        val layout = packets().single { it.getJSONObject("view").getLong("id") == id }.getJSONObject("pill")
        val corner = center(id) + Offset(layout.number("x"), layout.number("y")) * density
        val pillBounds = Rect(corner, androidx.compose.ui.geometry.Size(layout.number("width") * density, layout.number("height") * density))
        java.io.File(instrumentation.targetContext.getExternalFilesDir(null), "validation/tool-bubbles/$case-palette-bounds.json").writeText(obj("workspace" to area.toString(), "pill" to pillBounds.toString(), "packet" to layout, "nativeCenter" to bounds("tool-bubble-$id").center.toString(), "sharedCenter" to center(id).toString()).toString(2))
        shot("wheel-beads-pill")
        assertTrue("Palette pill and both 30dp buttons remain inside workspace: $pillBounds in $area", pillBounds.left >= area.left - 2 && pillBounds.top >= area.top - 2 && pillBounds.right <= area.right + 2 && pillBounds.bottom <= area.bottom + 2)
        val original = view(id).getString("pill")
        val pill = packets().single { it.getJSONObject("view").getLong("id") == id }.getJSONObject("pill")
        val at = center(id) + Offset(pill.number("x"), pill.number("y")) * density
        tap(at + Offset((pill.number("width") - 15) * density, 15 * density))
        waitFor("Next palette changes shared name") { view(id).getString("pill") != original }
        val next = view(id).getString("pill"); SystemClock.sleep(250)
        assertEquals("One Next contact advances once", next, view(id).getString("pill"))
        tap(at + Offset(15 * density, 15 * density))
        waitFor("Previous palette restores shared name") { view(id).getString("pill") == original }
        tap(at + Offset(pill.number("width") * density / 2, 15 * density))
        waitFor("Palette name opens its native menu") { findTag("workspace-menu") != null }
        escape()
        val bead = view(id).getJSONArray("beads").objects().firstOrNull { it.getJSONObject("placement").getJSONObject("bead").getString("kind") == "swatch" }
        assertNotNull("Palette exposes swatches", bead)
        val place = bead!!.getJSONObject("placement"); val beadPoint = place.getJSONArray("center")
        val start = center(id) + Offset(beadPoint.getDouble(0).toFloat(), beadPoint.getDouble(1).toFloat()) * density
        val before = wheelColor()
        stroke(start, start + Offset(0f, 25 * density), start + Offset(0f, 70 * density))
        assertEquals("Bead scrolling preserves the selected color", before, wheelColor())
    }
    private fun cardShot(id: Long, name: String) {
        val card = bounds("bubble-card-$id")
        val screen = IntArray(2)
        instrumentation.runOnMainSync { checkNotNull(findTag("workspace")).first.view.getLocationOnScreen(screen) }
        shot(name) { image ->
            var low = 255; var high = 0
            val left = (card.left + screen[0] + 10 * density).toInt().coerceIn(0, image.width - 1)
            val right = (card.right + screen[0] - 10 * density).toInt().coerceIn(left + 1, image.width)
            val top = (card.top + screen[1] + 10 * density).toInt().coerceIn(0, image.height - 1)
            val bottom = (card.bottom + screen[1] - 10 * density).toInt().coerceIn(top + 1, image.height)
            for (y in top until bottom step 3) for (x in left until right step 3) {
                val color = image.getPixel(x, y)
                val luminance = (android.graphics.Color.red(color) * 54 + android.graphics.Color.green(color) * 183 + android.graphics.Color.blue(color) * 19) / 256
                low = minOf(low, luminance); high = maxOf(high, luminance)
            }
            assertTrue("Native inspector presents controls rather than empty glass: $low..$high", high - low > 100)
        }
    }
    private fun dab(id: Long) {
        val size = view(id).getJSONArray("rings").objects().single { it.getString("setting") == "size" }
        event(MotionEvent.ACTION_DOWN, ringPoint(id, size, .2f)); event(MotionEvent.ACTION_MOVE, ringPoint(id, size, .82f))
        waitFor("Held arc publishes dab") { view(id).objectOrNull("stamp") != null }
        val stamp = view(id).getJSONObject("stamp"); val offset = stamp.getJSONArray("center")
        val at = center(id) + Offset(offset.getDouble(0).toFloat() + stamp.number("diameter") * .3f, offset.getDouble(1).toFloat()) * density
        var ink = false
        waitFor("Actual screenshot presents the enlarged dab") {
            shot("dab-presented") { image ->
                val pixel = image.getPixel(at.x.toInt().coerceIn(0, image.width - 1), at.y.toInt().coerceIn(0, image.height - 1))
                ink = android.graphics.Color.red(pixel) < 130 && android.graphics.Color.green(pixel) < 130 && android.graphics.Color.blue(pixel) < 130
            }; ink
        }
        for (fraction in listOf(.59f, .61f, .6f, .59f)) {
            event(MotionEvent.ACTION_MOVE, ringPoint(id, size, fraction)); host.drain()
            val text = view(id).getString("readout")
            var readout: SemanticsNode? = null
            instrumentation.runOnMainSync { readout = findNode(hasLabel(text))?.second }
            assertNotNull("Current readout is native text", readout)
            assertTrue("Readout width remains bounded", readout!!.boundsInRoot.width < 200 * density)
            assertBubbleLabel(id, "readout")
        }
        event(MotionEvent.ACTION_UP)
        waitFor("Releasing arc removes shared dab") { view(id).isNull("stamp") }
        SystemClock.sleep(ViewConfiguration.getDoubleTapTimeout().toLong() + 1)
        event(MotionEvent.ACTION_DOWN, ringPoint(id, size, .5f))
        waitFor("A later arc contact owns a fresh dab") { !view(id).isNull("stamp") }
        event(MotionEvent.ACTION_CANCEL); host.drain()
        waitFor("Cancelled arc retires dab") { view(id).isNull("stamp") }
    }
    private fun scrollSource() {
        val original = android.view.WindowManager.LayoutParams()
        val originalBounds = workspace()
        scenario.onActivity { original.copyFrom(it.window.attributes) }
        fun resize(width: Int, height: Int) {
            scenario.onActivity { it.window.setLayout((width * density).toInt(), (height * density).toInt()) }
            root = null
            waitFor("Private native viewport becomes $width x $height") {
                val bounds = workspace()
                kotlin.math.abs(bounds.width / density - width) < 2 && kotlin.math.abs(bounds.height / density - height) < 2
            }
        }
        try {
        val closed = take(2, originalBounds.center)
        repeat(4) { if (view(closed).getBoolean("open")) tap(stable(closed)) }
        assertFalse("Viewport fixture begins with a closed bubble", view(closed).getBoolean("open"))
        resize(500, 420)
        waitFor("Resizing wakes and clamps a closed native bubble") {
            val bounds = bounds("tool-bubble-$closed"); val area = workspace()
            bounds.left >= area.left && bounds.top >= area.top && bounds.right <= area.right && bounds.bottom <= area.bottom && (bounds.center - center(closed)).getDistance() < 2f
        }
        shot("closed-viewport-clamped")
        putBack(closed)
        tap(bounds("header-item-2").center)
        waitFor("Brush source drawer completes opening") { findTag("tool-drawer")?.second?.boundsInRoot?.height?.let { it > 120 * density } == true }
        var previousDrawer = bounds("tool-drawer")
        waitFor("Native source drawer arrangement settles") {
            SystemClock.sleep(50)
            val next = bounds("tool-drawer"); val same = next == previousDrawer; previousDrawer = next; same
        }
        fun scrollNode(): Pair<ViewRootForTest, SemanticsNode>? = findTag("tool-drawer")?.let { (r, n) ->
            n.find { it.config.getOrNull(SemanticsProperties.VerticalScrollAxisRange)?.maxValue?.invoke()?.let { max -> max > 0f } == true }?.let { r to it }
        }
        val scroll = scrollNode()
        val box = scroll?.second?.boundsInRoot ?: bounds("tool-drawer")
        if (scroll != null) {
        val before = scroll.second.config[SemanticsProperties.VerticalScrollAxisRange].value()
        root = scroll.first
        if (tool == MotionEvent.TOOL_TYPE_MOUSE) {
            val now = SystemClock.uptimeMillis()
            val e = MotionEvent.obtain(now, now, MotionEvent.ACTION_SCROLL, 1,
                arrayOf(MotionEvent.PointerProperties().apply { id = 0; toolType = tool }),
                arrayOf(MotionEvent.PointerCoords().apply { x = box.center.x; y = box.center.y; setAxisValue(MotionEvent.AXIS_VSCROLL, -5f) }),
                0, 0, 1f, 1f, 0, 0, android.view.InputDevice.SOURCE_MOUSE, 0)
            try { instrumentation.runOnMainSync { scroll.first.view.dispatchGenericMotionEvent(e) } } finally { e.recycle() }
        } else stroke(Offset(box.center.x, box.bottom - 20 * density), Offset(box.center.x, box.center.y), Offset(box.center.x, box.top + 20 * density))
        host.drain()
        java.io.File(instrumentation.targetContext.getExternalFilesDir(null), "validation/tool-bubbles/scroll-diagnostic.json").writeText(obj("tool" to tool, "before" to before, "bounds" to box.toString(), "drawer" to state().getJSONObject("customization").opt("drawer"), "after" to scrollNode()?.second?.config?.getOrNull(SemanticsProperties.VerticalScrollAxisRange)?.value?.invoke(), "contacts" to contacts).toString(2))
        waitFor("Native source motion scrolls", { "drawer=${state().getJSONObject("customization").opt("drawer")}; node=${scrollNode()?.second?.boundsInRoot}" }) { scrollNode()?.second?.config?.getOrNull(SemanticsProperties.VerticalScrollAxisRange)?.value?.invoke()?.let { it != before } == true }
        }
        assertTrue("Source scrolling never tears out a bubble", packets().isEmpty())
        var source: Pair<ViewRootForTest, SemanticsNode>? = null
        instrumentation.runOnMainSync {
            source = findTag("tool-drawer")?.let { (r, drawer) ->
                drawer.find { node -> node.config.getOrNull(SemanticsProperties.TestTag)?.startsWith("subtool-") == true &&
                    node.boundsInRoot.height > 30 * density && node.boundsInRoot.center.y > box.top + 10 * density && node.boundsInRoot.center.y < box.bottom - 10 * density }?.let { r to it }
            }
        }
        val row = checkNotNull(source) { "A scrolled brush row remains visible" }
        root = row.first
        val from = row.second.boundsInRoot.center
        val drawer = bounds("tool-drawer"); val area = workspace()
        val target = listOf(Offset(area.center.x, area.bottom - 2 * density), Offset(area.left + 2 * density, area.center.y), Offset(area.right - 2 * density, area.center.y))
            .firstOrNull { !drawer.contains(it) } ?: error("Short native drawer must leave a canvas tear-out target: $drawer in $area")
        stroke(from, from + Offset(30 * density, 0f), target)
        waitFor("Scrolled brush source tears out") { packets().any { it.getJSONObject("view").getString("kind") == "brush" } }
        root = null
        val id = packets().single { it.getJSONObject("view").getString("kind") == "brush" }.getJSONObject("view").getLong("id")
        assertTrue("Scrolled brush opens its halo", view(id).getBoolean("open"))
        val fromCenter = stable(id)
        stroke(fromCenter, workspace().bottomRight - Offset(10 * density, 10 * density))
        resize(400, 360)
        stable(id)
        val shrunk = bounds("tool-bubble-$id"); val shrunkArea = workspace()
        assertTrue("Shrinking viewport keeps bubble within canvas walls", shrunk.left >= shrunkArea.left && shrunk.top >= shrunkArea.top && shrunk.right <= shrunkArea.right && shrunk.bottom <= shrunkArea.bottom)
        screenshot("validation/tool-bubbles/viewport-$tool.png")
        stable(id); putBack(id)
        } finally {
            scenario.onActivity { it.window.attributes = original }
            root = null
            waitFor("Private native viewport restores both dimensions") {
                val restored = workspace(); kotlin.math.abs(restored.width - originalBounds.width) < 2 && kotlin.math.abs(restored.height - originalBounds.height) < 2
            }
        }
    }
    @Before fun ready() {
        scenario = launchCapy(90_000)
        scenario.onActivity { host = it.host; density = it.resources.displayMetrics.density }
        fixture = JSONObject(state().getJSONObject("workspace").toString()).apply { put("zen_mode", false) }
        val controls = listOf(obj("kind" to "command", "command" to "drawing_brush"), obj("kind" to "color"), obj("kind" to "color_picker"), obj("kind" to "command", "command" to "undo"), obj("kind" to "panel", "panel" to "layers"))
        fixture.getJSONObject("layout").apply {
            put("bubbles", obj("bubbles" to JSONArray(), "rings" to JSONArray(), "kept" to JSONArray()))
            put("header", obj("size" to "medium", "next_id" to 7, "zones" to JSONArray(listOf(JSONArray(listOf(obj("id" to 1, "item" to obj("kind" to "capy")))), JSONArray(controls.mapIndexed { i, control -> obj("id" to i + 2, "item" to obj("kind" to "tool", "control" to control)) }), JSONArray()))))
        }
    }
    @After fun close() {
        if (pressed) event(MotionEvent.ACTION_CANCEL)
        if (::scenario.isInitialized) scenario.close()
    }
    private fun assertHaloGeometry(id: Long) {
        val packet = packets().single { it.getJSONObject("view").getLong("id") == id }
        val v = packet.getJSONObject("view")
        val c = stable(id)
        SystemClock.sleep(250)
        val face = bounds("tool-bubble-$id")
        assertEquals("Face width follows shared geometry", v.getJSONArray("face").getDouble(0).toFloat(), face.width / density, 1f)
        assertEquals("Face height follows shared geometry", v.getJSONArray("face").getDouble(1).toFloat(), face.height / density, 1f)
        assertTrue("Face uses the shared center: $face at $c", (face.center - c).getDistance() < 2f)
        val halo = bounds("bubble-halo-$id")
        shot("halo-geometry-${v.getString("kind")}")
        val reach = (packet.number("reach") + 16f) * density
        val area = workspace()
        if (c.x - reach >= area.left && c.x + reach <= area.right && c.y - reach >= area.top && c.y + reach <= area.bottom)
            assertTrue("Halo and face share their center: halo=${halo.center}, face=${face.center}", (halo.center - face.center).getDistance() < 2f)
        packet.getJSONArray("rings").objects().forEachIndexed { i, ring ->
            val icon = bounds("bubble-ring-icon-$id-$i")
            val expected = c + ring.array("icon").bubblePoint() * density
            assertTrue("Ring $i icon is at the shared arc point: $icon, expected $expected", (icon.center - expected).getDistance() < 2f)
            assertEquals("Arc icon width", 16f, icon.width / density, 1f)
            assertEquals("Arc icon height", 16f, icon.height / density, 1f)
        }
    }
    private fun assertBubbleLabel(id: Long, part: String) {
        val layouts = mutableListOf<androidx.compose.ui.text.TextLayoutResult>()
        var surface: Rect? = null
        instrumentation.runOnMainSync {
            val node = checkNotNull(findTag("bubble-part-$id-$part")).second
            surface = node.boundsInRoot
            val text = checkNotNull(node.find { it.config.getOrNull(SemanticsActions.GetTextLayoutResult) != null })
            assertTrue(text.config[SemanticsActions.GetTextLayoutResult].action!!.invoke(layouts))
        }
        val text = layouts.single()
        val measured = text.size
        val packet = packets().single { it.getJSONObject("view").getLong("id") == id }
        val c = center(id)
        val expected = if (part == "badge") {
            assertEquals(androidx.compose.ui.text.font.FontWeight.SemiBold, text.layoutInput.style.fontWeight)
            val geometry = bubbleUi(obj("type" to "count_badge", "text" to JSONArray(listOf(measured.width / density, maxOf(22f, measured.height / density))))) as JSONArray
            geometry.getJSONArray(0).bubblePoint() to geometry.getJSONArray(1).bubblePoint()
        } else {
            assertEquals("tnum", text.layoutInput.style.fontFeatureSettings)
            packet.array("readout").bubblePoint() to Offset(measured.width / density + 14f, measured.height / density + 4f)
        }
        assertTrue("$part stays at its shared center: $surface", (surface!!.center - c - expected.first * density).getDistance() < 2f)
        assertEquals("$part fits its measured text width", expected.second.x * density, surface!!.width, 2f)
        assertEquals("$part fits its measured text height", expected.second.y * density, surface!!.height, 2f)
    }
    @Test fun bubbleHoverExitClearsFaceFeedback() {
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "restore_workspace", "workspace" to JSONObject(fixture.toString())))
            action(obj("type" to "set_theme", "theme" to theme))
            tool = MotionEvent.TOOL_TYPE_MOUSE
            val id = take(2, workspace().center)
            val target = bounds("tool-bubble-$id")
            val owner = checkNotNull(root)
            fun hover(phase: Int, position: Offset) {
                val e = motion(tool, phase, position, SystemClock.uptimeMillis(), button = 0)
                try { instrumentation.runOnMainSync { owner.view.dispatchGenericMotionEvent(e) } } finally { e.recycle() }
                SystemClock.sleep(120)
            }
            val origin = IntArray(2)
            instrumentation.runOnMainSync { owner.view.getLocationOnScreen(origin) }
            var frame = 0
            fun pixels(): IntArray {
                val shot = instrumentation.uiAutomation.takeScreenshot()
                val width = target.width.toInt(); val height = target.height.toInt()
                val crop = android.graphics.Bitmap.createBitmap(shot, origin[0] + target.left.toInt(), origin[1] + target.top.toInt(), width, height)
                val file = java.io.File(instrumentation.targetContext.getExternalFilesDir(null), "validation/bubble-feedback/$theme-${frame++}.png")
                file.parentFile!!.mkdirs()
                file.outputStream().use { crop.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) }
                crop.recycle()
                return IntArray(width * height).also {
                    shot.getPixels(it, 0, width, origin[0] + target.left.toInt(), origin[1] + target.top.toInt(), width, height)
                    shot.recycle()
                }
            }
            fun changed(a: IntArray, b: IntArray) = a.indices.count { i ->
                listOf(0, 8, 16).any { shift -> kotlin.math.abs((a[i] shr shift and 255) - (b[i] shr shift and 255)) > 8 }
            }
            hover(MotionEvent.ACTION_HOVER_ENTER, workspace().topLeft + Offset(10f, 100f))
            val idle = pixels()
            hover(MotionEvent.ACTION_HOVER_MOVE, target.center)
            val active = pixels()
            hover(MotionEvent.ACTION_HOVER_EXIT, target.center)
            val exited = pixels()
            assertTrue("Bubble face shows mouse hover", changed(idle, active) > idle.size / 50)
            assertTrue("$theme native hover exit clears bubble face: ${changed(idle, exited)}/${idle.size} pixels", changed(idle, exited) < idle.size / 50)
            hover(MotionEvent.ACTION_HOVER_ENTER, target.center)
            assertTrue("Bubble face shows hover before losing focus", changed(idle, pixels()) > idle.size / 50)
            var dialog: android.app.Dialog? = null
            try {
                scenario.onActivity { activity ->
                    dialog = android.app.Dialog(activity).apply {
                        setContentView(android.widget.TextView(activity).apply { text = "Focus fixture" })
                        show()
                        window!!.setGravity(android.view.Gravity.TOP or android.view.Gravity.START)
                        window!!.setLayout(120, 80)
                        window!!.clearFlags(android.view.WindowManager.LayoutParams.FLAG_DIM_BEHIND)
                        window!!.setDimAmount(0f)
                    }
                }
                waitFor("Native dialog takes bubble window focus") { !owner.view.hasWindowFocus() }
                SystemClock.sleep(120)
                assertTrue("$theme focus loss clears bubble face", changed(idle, pixels()) < idle.size / 50)
            } finally { instrumentation.runOnMainSync { dialog?.dismiss() } }
            waitFor("Bubble window regains native focus") { owner.view.hasWindowFocus() }
        }
    }

    @Test fun haloAndFaceShareTheCoreCenter() {
        for (theme in listOf("light", "dark")) {
            case = "$theme-geometry"
            action(obj("type" to "restore_workspace", "workspace" to JSONObject(fixture.toString())))
            action(obj("type" to "set_theme", "theme" to theme))
            val controls = take(4, workspace().center)
            assertHaloGeometry(controls)
            action(obj("type" to "bubble", "action" to obj("type" to "option", "id" to controls, "option" to obj("kind" to "swap"))))
            assertHaloGeometry(controls)
            invoke("lasso")
            assertTrue("Controls publish unavailable arcs for a tool without brush settings", view(controls).array("rings").objects().all { !it.getBoolean("enabled") })
            assertHaloGeometry(controls); shot("disabled-controls")
            putBack(controls)
            val brush = take(2, workspace().center)
            assertHaloGeometry(brush)
            val original = android.view.WindowManager.LayoutParams()
            val originalBounds = workspace()
            scenario.onActivity { original.copyFrom(it.window.attributes); it.window.setLayout((220 * density).toInt(), (420 * density).toInt()) }
            try {
                waitFor("Narrow viewport is measured") { kotlin.math.abs(workspace().width / density - 220f) < 2f }
                assertHaloGeometry(brush)
            } finally {
                scenario.onActivity { it.window.attributes = original }
                waitFor("Viewport restores after geometry check") { kotlin.math.abs(workspace().width - originalBounds.width) < 2f }
            }
        }
    }
    @Test fun countBadgesKeepTheirAnchorWhenTextWidens() {
        for (theme in listOf("light", "dark")) {
            case = "$theme-badge"
            action(obj("type" to "restore_workspace", "workspace" to JSONObject(fixture.toString())))
            action(obj("type" to "set_theme", "theme" to theme))
            val id = take(6, workspace().center)
            action(obj("type" to "bubble", "action" to obj("type" to "close", "id" to id)))
            waitFor("Layer card folds") { view(id).isNull("card") }
            stable(id); SystemClock.sleep(500)
            assertBubbleLabel(id, "badge")
            val before = state().getJSONArray("layers").length()
            repeat((12 - before).coerceAtLeast(0)) { action(obj("type" to "layer", "action" to obj("op" to "new", "group" to false, "clipped" to false))) }
            waitFor("Badge shows twelve layers") { view(id).getInt("layers") == maxOf(12, before) }
            stable(id); SystemClock.sleep(250)
            assertBubbleLabel(id, "badge"); shot("count-12")
            repeat((12 - before).coerceAtLeast(0)) { index -> invoke("undo"); waitFor("Count undo completes") { view(id).getInt("layers") == 11 - index } }
            stable(id); SystemClock.sleep(250)
            assertBubbleLabel(id, "badge")
        }
    }
    @Test fun hdrBubbleAcrossThemesSizesAndDevices() {
        host.createDrawing(640, 480, depth = "F16")
        waitFor("HDR drawing ready") { host.snapshot?.objectOrNull("color_panel")?.optBoolean("hdr") == true && host.snapshot?.optBoolean("brush_ready") == true }
        fun packet(id: Long) = packets().single { it.getJSONObject("view").getLong("id") == id }.getJSONObject("color")
        fun intensity() = host.snapshot!!.getJSONObject("color_panel").getDouble("intensity")
        fun arcPoint(id: Long, fraction: Float): Offset {
            val geometry = packet(id).getJSONObject("intensity").getJSONObject("geometry")
            val end = geometry.number("end_angle")
            val angle = Math.PI.toFloat() - end - fraction * (Math.PI.toFloat() - 2 * end)
            return center(id) + (geometry.array("center").bubblePoint() + Offset(cos(angle), sin(angle)) * geometry.number("radius")) * density
        }
        for (theme in listOf("light", "dark")) {
            case = "hdr-$theme"; tool = MotionEvent.TOOL_TYPE_MOUSE
            action(obj("type" to "restore_workspace", "workspace" to JSONObject(fixture.toString())))
            action(obj("type" to "set_theme", "theme" to theme))
            val id = take(3, workspace().center)
            for (size in listOf(136, 216, 344)) for ((palette, pair) in listOf(false to false, false to true, true to true)) {
                action(obj("type" to "bubble", "action" to obj("type" to "set_options", "id" to id, "options" to obj(
                    "size" to size, "recent" to true, "palette" to palette, "pair" to pair, "full" to false, "fold" to true, "swap" to false))))
                stable(id); SystemClock.sleep(250)
                val color = packet(id)
                val face = bounds("tool-bubble-$id")
                assertEquals("Wheel keeps its selected diameter", size.toFloat(), view(id).number("diameter"), .01f)
                assertTrue("Native HDR plate includes its lower arc", face.height / density > size)
                assertTrue("Shared HDR outline has rounded segments", color.array("outline").length() >= 4)
                assertTrue("EV arc remains in the native bubble", findTag("tool-bubble-$id")!!.second.find(hasTag("color-hdr-intensity")) != null)
                for (fraction in listOf(0f, .5f, 1f)) {
                    val local = (arcPoint(id, fraction) - center(id)) / density
                    assertEquals("Visible EV caps own native contact", "intensity", query(obj("type" to "bubble_hit", "id" to id, "x" to local.x, "y" to local.y)))
                }
                shot("size-$size-palette-$palette-pair-$pair")
            }
            action(obj("type" to "bubble", "action" to obj("type" to "option", "id" to id, "option" to obj("kind" to "size", "size" to 216))))
            stable(id)
            for (input in listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
                tool = input
                action(obj("type" to "color", "action" to obj("op" to "hdr_intensity", "stops" to 0)))
                SystemClock.sleep(ViewConfiguration.getDoubleTapTimeout().toLong() + 1)
                stroke(arcPoint(id, .125f), arcPoint(id, .375f), arcPoint(id, .625f))
                waitFor("$theme/$input EV gesture commits") { kotlin.math.abs(intensity() - 3.0) < .08 }
                val before = intensity()
                event(MotionEvent.ACTION_DOWN, arcPoint(id, .25f)); event(MotionEvent.ACTION_MOVE, arcPoint(id, .5f))
                event(MotionEvent.ACTION_CANCEL); host.drain()
                assertEquals("Cancelled EV gesture restores authored intensity", before, intensity(), .01)
                val c = center(id)
                stroke(c + Offset(12 * density, -8 * density), c + Offset(-12 * density, 8 * density))
                assertEquals("Wheel edits preserve EV", before, intensity(), .01)
                SystemClock.sleep(ViewConfiguration.getDoubleTapTimeout().toLong() + 1)
                doubleTap(arcPoint(id, .5f), arcPoint(id, .5f))
                waitFor("Double contact resets EV") { intensity() == 0.0 }
                val rim = center(id) - Offset(0f, view(id).number("diameter") * density / 2 - 2 * density)
                val old = center(id)
                stroke(rim, rim + Offset(20 * density, 0f), rim + Offset(40 * density, 0f))
                assertTrue("HDR rim drags the complete plate", (stable(id) - old).getDistance() > 20 * density)
                shot("input-$input")
            }
            tool = MotionEvent.TOOL_TYPE_FINGER
            twoTouches(id)
            invoke("quick_mask")
            waitFor("Quick Mask retires the HDR arc") { packets().single { it.getJSONObject("view").getLong("id") == id }.isNull("color") }
            invoke("quick_mask")
            waitFor("Leaving Quick Mask restores HDR geometry") { !packets().single { it.getJSONObject("view").getLong("id") == id }.isNull("color") }
            shot("review")
        }
    }
    @Test fun colorRingTapsAndMenuUndoColor() {
        fun foreground() = state().getJSONObject("colors").getJSONObject("foreground").getJSONArray("rgba").toString()
        fun dispatch(event: MotionEvent) {
            try { instrumentation.runOnMainSync { checkNotNull(root).view.dispatchTouchEvent(event) } } finally { event.recycle() }
        }
        fun fingerTap(points: List<Offset>) {
            val start = SystemClock.uptimeMillis()
            instrumentation.runOnMainSync { root = checkNotNull(findTag("workspace")).first }
            fun send(action: Int, count: Int) = dispatch(MotionEvent.obtain(start, SystemClock.uptimeMillis(), action, count,
                (0 until count).map { MotionEvent.PointerProperties().apply { id = it; toolType = MotionEvent.TOOL_TYPE_FINGER } }.toTypedArray(),
                (0 until count).map { MotionEvent.PointerCoords().apply { x = points[it].x; y = points[it].y; pressure = .7f } }.toTypedArray(),
                0, 0, 1f, 1f, 0, 0, android.view.InputDevice.SOURCE_TOUCHSCREEN, 0))
            send(MotionEvent.ACTION_DOWN, 1)
            for (i in 1 until points.size) send(MotionEvent.ACTION_POINTER_DOWN or (i shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), i + 1)
            for (i in points.size - 1 downTo 1) send(MotionEvent.ACTION_POINTER_UP or (i shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), i + 1)
            send(MotionEvent.ACTION_UP, 1)
            root = null; host.drain()
        }
        for (theme in listOf("light", "dark")) {
            case = "undo-$theme"; tool = MotionEvent.TOOL_TYPE_FINGER
            action(obj("type" to "restore_workspace", "workspace" to JSONObject(fixture.toString())))
            action(obj("type" to "set_theme", "theme" to theme))
            val id = take(3, workspace().center)
            val c = stable(id)
            val before = foreground()
            stroke(c + Offset(12 * density, -8 * density), c + Offset(-12 * density, 8 * density))
            val picked = foreground()
            assertNotEquals("$theme: the ring picks", before, picked)
            val pair = listOf(c - Offset(25 * density, 0f), c + Offset(25 * density, 0f))
            fingerTap(pair)
            waitFor("$theme: a two-finger tap on the ring undoes") { foreground() == before }
            assertTrue("$theme: and leaves the ring where it is", (stable(id) - c).getDistance() < 1f)
            fingerTap(pair + (c + Offset(0f, 30 * density)))
            waitFor("$theme: a three-finger tap on the ring redoes") { foreground() == picked }
            val undo = menuItems(id).first { it.optJSONObject("action")?.optJSONObject("action")?.optString("op") == "undo" }.getString("label")
            instrumentation.runOnMainSync { root = checkNotNull(findTag("workspace")).first }
            val down = SystemClock.uptimeMillis()
            for (step in listOf(MotionEvent.ACTION_DOWN, MotionEvent.ACTION_UP)) dispatch(motion(MotionEvent.TOOL_TYPE_MOUSE, step, c, down, MotionEvent.BUTTON_SECONDARY))
            root = null
            waitFor("$theme: right-clicking the ring opens its menu") { findTag("workspace-menu") != null }
            shot("menu")
            tapMenu(undo)
            waitFor("$theme: the ring's menu undoes") { foreground() == before }
            putBack(id)
        }
    }
    @Test fun nativeBubblePickupClosesOpenDrawers() {
        for (theme in listOf("light", "dark")) for (input in listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
            tool = input; root = null; case = "drawer-$theme-$input"
            for (header in listOf(3, 6, 2)) {
                action(obj("type" to "restore_workspace", "workspace" to JSONObject(fixture.toString())))
                action(obj("type" to "set_theme", "theme" to theme))
                val from = bounds("header-item-$header").center
                tap(from)
                waitFor("Source drawer opens") { findTag("tool-drawer") != null }
                val before = packets().map { it.getJSONObject("view").getLong("id") }.toSet()
                val target = workspace().center
                event(MotionEvent.ACTION_DOWN, from)
                event(MotionEvent.ACTION_MOVE, from + Offset(0f, 8 * density))
                event(MotionEvent.ACTION_MOVE, from + Offset(0f, 80 * density))
                event(MotionEvent.ACTION_MOVE, target)
                waitFor("Pickup closes the source drawer before release") { state().getJSONObject("customization").isNull("drawer") && findTag("tool-drawer") == null }
                val id = packets().single { it.getJSONObject("view").getLong("id") !in before }.getJSONObject("view").getLong("id")
                val continued = target + Offset(35 * density, 25 * density)
                event(MotionEvent.ACTION_MOVE, continued); event(MotionEvent.ACTION_UP); host.drain()
                assertTrue("Captured pickup survives drawer dismissal", packets().any { it.getJSONObject("view").getLong("id") == id })
                assertTrue("Pickup continues after drawer dismissal", (stable(id) - continued).getDistance() < 30 * density)
                shot("header-$header")
            }
        }
    }
    @Test fun sketchEraserBubbleKeepsItsHaloChooserAndSettingsAcrossThemesAndDevices() {
        val tags=host.bootstrap!!.array("shipped_tags").values()
        action(obj("type" to "preferences","action" to obj("type" to "edit","id" to "language","value" to tags.indexOf("en")+1)))
        waitFor("English Eraser bubble fixture") { host.languageTag=="en" }
        val sketch=JSONObject(state().getJSONObject("workspace").toString()).apply { put("zen_mode",false) }
        val header=sketch.getJSONObject("layout").getJSONObject("header").array("zones").values()
            .flatMap { (it as JSONArray).objects() }.single {
                it.getJSONObject("item").objectOrNull("control")?.optString("command")=="eraser"
            }.getInt("id")
        val preset=host.catalog.array("brush_categories").objects().single { it.getString("id")=="eraser" }
            .array("brushes").objects().single().getLong("id")
        val arguments=androidx.test.platform.app.InstrumentationRegistry.getArguments()
        val themes=arguments.getString("bubbleTheme")?.let { listOf(it) } ?: listOf("light","dark")
        val inputs=arguments.getString("bubbleInput")?.let { name -> listOf(when(name) {
            "mouse" -> MotionEvent.TOOL_TYPE_MOUSE; "touch" -> MotionEvent.TOOL_TYPE_FINGER; "pen" -> MotionEvent.TOOL_TYPE_STYLUS
            else -> error("Unknown bubble input $name")
        }) } ?: listOf(MotionEvent.TOOL_TYPE_MOUSE,MotionEvent.TOOL_TYPE_FINGER,MotionEvent.TOOL_TYPE_STYLUS)
        for(theme in themes) for(input in inputs) {
            tool=input;root=null;case="sketch-eraser-$theme-$input"
            action(obj("type" to "restore_workspace","workspace" to JSONObject(sketch.toString())))
            action(obj("type" to "set_theme","theme" to theme))
            invoke("pen")
            val target=workspace().let { Offset(it.left+it.width*.4f,it.top+it.height*.5f) }
            val id=take(header,target)
            val initial=view(id)
            assertEquals("dial",initial.getString("kind"))
            assertEquals("eraser",initial.getString("icon"))
            assertTrue(initial.getBoolean("selected") && initial.getBoolean("open"))
            assertEquals(preset,state().getJSONObject("brush").getLong("preset"))
            assertTrue("Eraser opens its native slider halo",initial.array("rings").length()>0)
            assertEquals(0,initial.array("chooser").length())
            assertHaloGeometry(id)
            val ring=initial.array("rings").objects().first()
            val key=ring.getString("setting");val before=setting(key)
            val from=ring.getDouble("fill").toFloat();val ringTarget=if(from<.5f) .8f else .2f
            stroke(ringPoint(id,ring,from),ringPoint(id,ring,(from+ringTarget)/2),ringPoint(id,ring,ringTarget))
            waitFor("Eraser native arc changes $key") { setting(key)!=before }
            val changed=setting(key)
            tap(stable(id))
            waitFor("Eraser dial opens its sole leaf") { view(id).array("chooser").length()==1 }
            val choice=view(id).array("chooser").objects().single()
            assertEquals("Eraser",choice.getString("label"))
            assertTrue(choice.getBoolean("selected") && choice.getBoolean("enabled"))
            val at=choice.array("center")
            tap(center(id)+Offset(at.getDouble(0).toFloat(),at.getDouble(1).toFloat())*density)
            waitFor("Eraser choice keeps its slider halo") { view(id).array("chooser").length()==0 && view(id).array("rings").length()>0 }
            assertEquals(preset,state().getJSONObject("brush").getLong("preset"))
            assertEquals(changed,setting(key))
            invoke("pen");tap(stable(id))
            waitFor("Eraser bubble reactivates its remembered tool") { state().getJSONObject("brush").getLong("preset")==preset && view(id).getBoolean("selected") }
            assertEquals(changed,setting(key))
            shot("halo-and-choice")
            putBack(id)
            assertTrue("Eraser returns to its native Sketch header",packets().isEmpty())
        }
    }
    @Test fun nativeBubblesAcrossThemesAndDevices() {
        val arguments = androidx.test.platform.app.InstrumentationRegistry.getArguments()
        val themes = arguments.getString("bubbleTheme")?.let { listOf(it) } ?: listOf("light", "dark")
        val inputs = arguments.getString("bubbleInput")?.let { name -> listOf(when (name) { "mouse" -> MotionEvent.TOOL_TYPE_MOUSE; "touch" -> MotionEvent.TOOL_TYPE_FINGER; "pen" -> MotionEvent.TOOL_TYPE_STYLUS; else -> error("Unknown bubble input $name") }) }
            ?: listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)
        for (theme in themes) for (input in inputs) {
            tool = input; root = null; case = "$theme-$input"
            instrumentation.sendStatus(0, android.os.Bundle().apply { putString("stream", "Tool bubbles: $theme/$input\n") })
            action(obj("type" to "restore_workspace", "workspace" to JSONObject(fixture.toString())))
            action(obj("type" to "set_theme", "theme" to theme))
            val target = workspace().let { Offset(it.left + it.width * .4f, it.top + it.height * .5f) }
            val emptyHistory = take(5, target)
            stable(emptyHistory); SystemClock.sleep(250); shot("history-empty")
            putBack(emptyHistory)
            val brush = take(2, target)
            var v = view(brush)
            assertTrue("Tear-out selects and opens the shared halo", v.getBoolean("selected") && v.getBoolean("open") && v.getJSONArray("rings").length() > 0)
            val c = stable(brush)
            val gap = (0 until 360 step 3).map { degrees ->
                val radians = Math.toRadians(degrees.toDouble())
                Offset(cos(radians).toFloat(), sin(radians).toFloat()) * 55f
            }.firstOrNull { local -> query(obj("type" to "bubble_hit", "id" to brush, "x" to local.x, "y" to local.y)) == null }
            assertNotNull("Shared halo hit geometry leaves canvas gaps", gap)
            val revision = state().getJSONArray("layers").objects().first { it.optBoolean("editing") }.getLong("paint_revision")
            val gapPoint = c + checkNotNull(gap) * density
            val savedTool = tool; tool = MotionEvent.TOOL_TYPE_STYLUS
            tap(gapPoint); tool = savedTool
            waitFor("Halo gap admits pen painting") { state().getJSONArray("layers").objects().first { it.optBoolean("editing") }.getLong("paint_revision") > revision }
            invoke("undo")
            SystemClock.sleep(360)
            val preservedLayer = state().getJSONArray("layers").objects().first { it.optBoolean("editing") }
            val preservedId = preservedLayer.getLong("id"); val preservedRevision = preservedLayer.getLong("paint_revision")
            fun unchangedPaint(stage: String) = assertEquals("$stage must not paint through the native UI", preservedRevision, state().getJSONArray("layers").objects().single { it.getLong("id") == preservedId }.getLong("paint_revision"))
            assertTrue("Painting through a gap keeps the halo open", view(brush).getBoolean("open"))
            val nativeCenter = bounds("tool-bubble-$brush").center
            assertTrue("Shared center $c must match native center $nativeCenter; workspace=${workspace()}; density=$density; placements=${placements()}", (c - nativeCenter).getDistance() < 2f)
            val rings = v.getJSONArray("rings").objects()
            val ring = rings.first()
            val setting = ring.getString("setting")
            val previous = setting(setting)
            stroke(ringPoint(brush, ring, .1f), ringPoint(brush, ring, .4f), ringPoint(brush, ring, .8f))
            waitFor("Arc changes $setting") { setting(setting) != previous }
            val committed = setting(setting)
            SystemClock.sleep(ViewConfiguration.getDoubleTapTimeout().toLong() + 1)
            event(MotionEvent.ACTION_DOWN, ringPoint(brush, ring, .3f)); event(MotionEvent.ACTION_MOVE, ringPoint(brush, ring, .6f))
            pressKey(android.view.KeyEvent.KEYCODE_ESCAPE); event(MotionEvent.ACTION_UP)
            waitFor("Escape restores $setting to $committed, current=${setting(setting)}") { setting(setting) == committed }
            action(obj("type" to "reset_tool_setting", "id" to setting))
            val defaultValue = setting(setting)
            SystemClock.sleep(ViewConfiguration.getDoubleTapTimeout().toLong() + 1)
            stroke(ringPoint(brush, ring, .1f), ringPoint(brush, ring, .2f))
            val resetPoint = ringPoint(brush, ring, .5f)
            SystemClock.sleep(ViewConfiguration.getDoubleTapTimeout().toLong() + 1)
            doubleTap(resetPoint, resetPoint + Offset(2 * density, 0f))
            waitFor("Double contact resets the shared ring default") { setting(setting) == defaultValue }
            unchangedPaint("Ring edits and resets")
            invoke("lasso"); tap(stable(brush))
            waitFor("First contact selects and opens sliders") { view(brush).getBoolean("selected") && view(brush).getJSONArray("rings").length() > 0 }
            SystemClock.sleep(250); shot("halo")
            unchangedPaint("Select and halo contacts")
            tap(stable(brush)); waitFor("Next contact opens chooser") { view(brush).getJSONArray("chooser").length() > 0 }
            SystemClock.sleep(500); shot("chooser")
            val medium = view(brush).getJSONArray("chooser").objects().first { it.getString("kind") == "medium" && it.getBoolean("enabled") }
            fun choose(item: JSONObject) { val p = item.getJSONArray("center"); tap(center(brush) + Offset(p.getDouble(0).toFloat(), p.getDouble(1).toFloat()) * density) }
            choose(medium)
            waitFor("Medium shows brush fan") { view(brush).getJSONArray("chooser").objects().any { it.getString("kind") == "brush" } }
            val beforePreset = state().getJSONObject("brush").getLong("preset")
            val choices = view(brush).getJSONArray("chooser").objects().filter { it.getString("kind") == "brush" && it.getBoolean("enabled") }
            val intended = choices.firstOrNull { it.optLong("preview", -1) != beforePreset } ?: choices.first()
            choose(intended)
            waitFor("Choosing intended brush updates shared preset") { state().getJSONObject("brush").getLong("preset") == intended.getLong("preview") }
            unchangedPaint("Chooser choice")
            waitFor("Choosing brush closes chooser") { view(brush).getJSONArray("chooser").length() == 0 }
            invoke("zen_mode"); val zenCenter = stable(brush)
            stroke(zenCenter, zenCenter + Offset(30 * density, 15 * density), zenCenter + Offset(60 * density, 30 * density))
            assertTrue("Native face drag moves in Zen", (stable(brush) - zenCenter).getDistance() > 30 * density)
            invoke("zen_mode")
            unchangedPaint("Zen face drag")
            shot("zen-face-moved")
            putBack(brush)
            invoke("undo_workspace")
            waitFor("Undo returns bubble") { packets().any { it.getJSONObject("view").getLong("id") == brush } }
            invoke("redo_workspace")
            waitFor("Redo returns source home") { packets().isEmpty() }
            action(obj("type" to "select_brush", "id" to 1))
            action(obj("type" to "set_color", "rgba" to JSONArray(listOf(.1, .1, .1, 1))))
            val controls = take(4, target)
            assertHaloGeometry(controls)
            dab(controls)
            if (input == MotionEvent.TOOL_TYPE_FINGER) secondFingerRestoresRing(controls)
            val replacementRing = view(controls).getJSONArray("rings").objects().first()
            event(MotionEvent.ACTION_DOWN, ringPoint(controls, replacementRing, .3f))
            action(obj("type" to "restore_workspace", "workspace" to JSONObject(fixture.toString())))
            event(MotionEvent.ACTION_UP)
            waitFor("Replacing source layout retires captured contact") { packets().isEmpty() }
            assertTrue("Replacement leaves the native host healthy", state().isNull("host_error"))
            action(obj("type" to "set_color", "rgba" to JSONArray(listOf(.8, .15, .1, 1))))
            val color = take(3, target)
            val originalColor = wheelColor()
            val wheelCenter = stable(color)
            val wheelPoint = wheelCenter + Offset(view(color).number("diameter") * .44f * density, 0f)
            val beforeWheel = obj("color" to originalColor, "view" to view(color), "nativeCenter" to bounds("tool-bubble-$color").center.toString(), "sharedCenter" to wheelCenter.toString(), "point" to wheelPoint.toString(), "motion" to query(obj("type" to "bubble_motion", "now_ns" to System.nanoTime())), "hit" to query(obj("type" to "bubble_hit", "id" to color, "x" to view(color).number("diameter") * .44f, "y" to 0)))
            tap(wheelPoint)
            java.io.File(instrumentation.targetContext.getExternalFilesDir(null), "validation/tool-bubbles/wheel-diagnostic.json").writeText(obj("before" to beforeWheel, "afterColor" to wheelColor(), "afterView" to view(color), "afterMotion" to query(obj("type" to "bubble_motion", "now_ns" to System.nanoTime())), "contacts" to contacts).toString(2))
            waitFor("Rapid wheel contact picks color") { wheelColor() != originalColor }
            val pickedColor = wheelColor()
            val diameter = view(color).number("diameter") * density
            stroke(wheelCenter, wheelCenter + Offset(diameter * 1.2f, 0f), wheelCenter + Offset(diameter * 1.3f, 30 * density))
            assertEquals("Wheel drag-out restores its press color", pickedColor, wheelColor())
            if (input == MotionEvent.TOOL_TYPE_FINGER) twoTouches(color)
            if (input == MotionEvent.TOOL_TYPE_STYLUS) twoTouches(color, MotionEvent.TOOL_TYPE_STYLUS)
            hold(color)
            val options = menuItems(color).filter { it.objectOrNull("action")?.objectOrNull("action")?.optString("type") == "option" }.take(2)
            assertEquals(2, options.size)
            for (option in options) {
                tapMenu(option.getString("label"))
                waitFor("Option toggles ${option.getString("label")}") { menuItems(color).single { it.optString("label") == option.getString("label") }.getBoolean("selected") != option.getBoolean("selected") }
                assertNotNull("Options keep the native menu open", findTag("workspace-menu"))
            }
            options.forEach { option -> assertEquals(!option.getBoolean("selected"), menuItems(color).single { it.optString("label") == option.getString("label") }.getBoolean("selected")) }
            shot("menu-options")
            escape(); stable(color); SystemClock.sleep(250); paletteControls(color)
            hold(color)
            action(obj("type" to "restore_workspace", "workspace" to JSONObject(fixture.toString())))
            waitFor("Removing a menu owner dismisses its native menu") { findTag("workspace-menu") == null && packets().isEmpty() }
            val history = take(5, target)
            SystemClock.sleep(250); shot("history-initial")
            val beforeLayers = state().getJSONArray("layers").length()
            action(obj("type" to "layer", "action" to obj("op" to "new", "group" to false, "clipped" to false)))
            SystemClock.sleep(250); shot("history-redo-disabled")
            tap(center(history) + Offset(-24 * density, 0f))
            waitFor("History bubble native Undo") { state().getJSONArray("layers").length() == beforeLayers }
            tap(center(history) + Offset(24 * density, 0f))
            waitFor("History bubble native Redo") { state().getJSONArray("layers").length() == beforeLayers + 1 }
            invoke("undo"); putBack(history)
            val layers = take(6, target)
            waitFor("Layers tears out as a simple card") { view(layers).objectOrNull("card")?.optBoolean("full") == false }
            waitFor("Native card completes its opening geometry", { "native=${bounds("bubble-card-$layers")}; density=$density; shared=${view(layers).getJSONObject("card")}" }) {
                val size = view(layers).getJSONObject("card").getJSONArray("size"); val bounds = bounds("bubble-card-$layers")
                kotlin.math.abs(bounds.width - size.getDouble(0) * density) < 2 && kotlin.math.abs(bounds.height - size.getDouble(1) * density) < 2
            }
            cardShot(layers, "layers-simple")
            instrumentation.runOnMainSync {
                val simple = checkNotNull(findTag("bubble-card-$layers")).second
                assertNull("Simple card has no row grips", simple.find { it.config.getOrNull(SemanticsProperties.TestTag)?.startsWith("layer-grip-") == true })
                assertNull("Simple card has no load-selection controls", simple.find { it.config.getOrNull(SemanticsProperties.TestTag)?.startsWith("selection-load-") == true })
            }
            if (input == MotionEvent.TOOL_TYPE_FINGER) secondFingerTakesLayerRow(layers)
            val layerCount = state().getJSONArray("layers").length()
            val footer = bounds("bubble-new-layer-$layers"); val cardCenter = center(layers)
            val simpleBounds = bounds("bubble-card-$layers")
            val control = host.catalog.getJSONObject("panel_metrics").getDouble("control_height").toFloat()
            assertEquals("Simple Layers footer's add cell is its slot and 6 dp sides", control + 12f, footer.width / density, 1f)
            val firstRow = state().getJSONArray("layers").getJSONObject(0).getLong("id")
            fun inCard(tag: String): Rect { var r: Rect? = null; instrumentation.runOnMainSync { r = findTag("bubble-card-$layers")?.second?.find(hasTag(tag))?.boundsInRoot }; return checkNotNull(r) { "Missing $tag in the Layers card" } }
            assertEquals("Simple Layers footer's New Layer slot sits 6 dp in, like the eye", simpleBounds.left + 6 * density, inCard("bubble-new-layer-slot-$layers").left, 1f)
            assertEquals("Simple Layers chevron sits 6 dp from the right edge", simpleBounds.right - 6 * density, inCard("layer-properties-chevron-slot").right, 1f)
            assertEquals("Simple Layers rows run edge to edge", simpleBounds.width, inCard("layer-row-$firstRow").width, 1f)
            assertEquals("Simple Layers eye sits 6 dp in", simpleBounds.left + 6 * density, inCard("layer-eye-$firstRow").left, 1f)
            val footerLocal = (footer.center - cardCenter) / density
            val footerHit = query(obj("type" to "bubble_hit", "id" to layers, "x" to footerLocal.x, "y" to footerLocal.y))
            val cardBefore = view(layers); val nativeCardBounds = bounds("bubble-card-$layers"); val beforeMotion = query(obj("type" to "bubble_motion", "now_ns" to System.nanoTime()))
            tap(footer.center)
            java.io.File(instrumentation.targetContext.getExternalFilesDir(null), "validation/tool-bubbles/card-footer-diagnostic.json").writeText(obj("footerBounds" to footer.toString(), "nativeCardBounds" to nativeCardBounds.toString(), "beforeMotion" to beforeMotion, "afterMotion" to query(obj("type" to "bubble_motion", "now_ns" to System.nanoTime())), "layerCountBefore" to layerCount, "layerCountAfter" to state().getJSONArray("layers").length(), "density" to density, "center" to cardCenter.toString(), "local" to footerLocal.toString(), "hit" to footerHit, "before" to cardBefore, "after" to view(layers), "contacts" to contacts).toString(2))
            waitFor("Simple card uses the native layer footer") { state().getJSONArray("layers").length() == layerCount + 1 }
            var card: SemanticsNode? = null
            instrumentation.runOnMainSync { card = findTag("bubble-card-$layers")?.second }
            tap(bounds("bubble-properties-$layers").center)
            waitFor("Card switches to native Properties") { view(layers).getJSONObject("card").getBoolean("properties") }
            SystemClock.sleep(250); cardShot(layers, "layers-properties")
            tap(bounds("bubble-properties-$layers").center)
            waitFor("Card switches back to Layers") { !view(layers).getJSONObject("card").getBoolean("properties") }
            instrumentation.runOnMainSync { assertEquals("Layers card keeps its retained owner", card?.id, findTag("bubble-card-$layers")?.second?.id) }
            invoke("undo")
            waitFor("Card layer addition is one undo") { state().getJSONArray("layers").length() == layerCount }
            val cardBounds = bounds("bubble-card-$layers")
            val blank = Offset(cardBounds.right - 3 * density, cardBounds.center.y)
            stroke(blank, blank + Offset(-30 * density, 0f), blank + Offset(-60 * density, -15 * density))
            assertTrue("Blank card edge moves its owner", (stable(layers) - cardBounds.center).getDistance() > 25 * density)
            event(MotionEvent.ACTION_DOWN, Offset(bounds("bubble-card-$layers").right - 3 * density, bounds("bubble-card-$layers").center.y))
            SystemClock.sleep(ViewConfiguration.getLongPressTimeout().toLong() + 250); event(MotionEvent.ACTION_UP)
            waitFor("Held card opens menu") { findTag("workspace-menu") != null }
            val full = menuItems(layers).single { it.objectOrNull("action")?.objectOrNull("action")?.objectOrNull("option")?.optBoolean("full") == true }
            tapMenu(full.getString("label")); escape()
            waitFor("Full uses native Layers inspector") { view(layers).getJSONObject("card").getBoolean("full") }
            waitFor("Full inspector exposes row grips") {
                var found = false
                instrumentation.runOnMainSync { found = findTag("bubble-card-$layers")?.second?.find { it.config.getOrNull(SemanticsProperties.TestTag)?.startsWith("layer-grip-") == true } != null }
                found
            }
            val fullCaptions = JSONArray()
            fun fullCaption(open: Boolean) {
                val expected = state().getJSONObject("layer_properties").getJSONObject("disclosure").getString(if (open) "return_label" else "label")
                var actual: String? = null
                waitFor("Full footer uses the shared ${if (open) "return" else "disclosure"} caption", { "expected=$expected actual=$actual" }) {
                    instrumentation.runOnMainSync { actual = findTag("bubble-card-$layers")?.second?.find(hasTag("layer-properties-disclosure-label"))?.config?.getOrNull(SemanticsProperties.Text)?.singleOrNull()?.text }
                    actual == expected
                }
                fullCaptions.put(obj("properties" to open, "expected" to expected, "actual" to actual))
            }
            fullCaption(false)
            tap(bounds("bubble-properties-$layers").center)
            waitFor("Full opens its native Properties page") { view(layers).getJSONObject("card").getBoolean("properties") }
            fullCaption(true)
            SystemClock.sleep(250); cardShot(layers, "layers-full-properties")
            tap(bounds("bubble-properties-$layers").center)
            waitFor("Full returns to native Layers") { !view(layers).getJSONObject("card").getBoolean("properties") }
            fullCaption(false)
            java.io.File(instrumentation.targetContext.getExternalFilesDir(null), "validation/tool-bubbles/$case-full-captions.json").writeText(fullCaptions.toString(2))
            SystemClock.sleep(250); cardShot(layers, "layers-full")
            val retiringFooter = bounds("bubble-properties-$layers").center
            val layersBeforeFold = state().getJSONArray("layers").length()
            tap(workspace().let { Offset(it.right - 30 * density, it.top + 100 * density) })
            waitFor("Outside contact folds card") { view(layers).isNull("card") }
            waitFor("Native card retires its interactive subtree", { "published=${host.snapshot?.optJSONArray("bubbles")}; core=${packets()}" }) { findTag("bubble-card-$layers") == null }
            tap(retiringFooter)
            assertEquals("Exiting card footer cannot mutate layers", layersBeforeFold, state().getJSONArray("layers").length())
            waitFor("Folded native face replaces the exiting card") { findTag("tool-bubble-$layers") != null && findTag("bubble-card-$layers") == null }
            shot("layers-folded")
            SystemClock.sleep(360)
            putBack(layers)
            assertTrue("All sources return home", packets().isEmpty())
            replacingCandidate(target)
            rapidHeaderTake(target)
            heldTile(target)
            scrollSource()
            instrumentation.sendStatus(0, android.os.Bundle().apply { putString("stream", "PASS tool bubbles: $theme/$input\n") })
        }
    }
}
