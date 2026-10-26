package art.capycanvas

import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.asAndroidPath
import androidx.compose.ui.graphics.addOutline
import androidx.compose.ui.graphics.Outline
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.asComposePath
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.LayoutDirection
import org.json.JSONObject

internal val LocalVectorPanel = staticCompositionLocalOf { false }

internal class PanelVectorShape(private val outline: Outline, val triangles: List<FloatArray> = emptyList(), bodyOutline: Outline = outline) : Shape {
    val body = object : Shape {
        override fun createOutline(size: Size, layoutDirection: LayoutDirection, density: Density) = bodyOutline
    }
    override fun createOutline(size: Size, layoutDirection: LayoutDirection, density: Density) = outline
}

internal data class PanelVectorColumn(val id: Int, val groups: Set<Int>, val bounds: JSONObject, val shape: PanelVectorShape)
internal data class PanelVectorGeometry(val surfaces: Map<Int, PanelVectorShape>, val columns: List<PanelVectorColumn>)

internal class PanelVectorCache {
    private data class Key(val owner: Long, val group: Int, val generation: Long, val scale: Float)
    private data class Entry(val key: Key, val shape: PanelVectorShape, val weight: Int)
    private data class PlacedShape(val group: Int, val generation: Long, val x: Float, val y: Float)
    private data class ColumnKey(val id: Int, val owner: Long, val source: List<Float>, val members: List<PlacedShape>, val scale: Float,
        val allocation: androidx.compose.ui.geometry.Rect)
    private data class ColumnEntry(val column: PanelVectorColumn, val weight: Int)
    private val paths = object : android.util.LruCache<Key, Entry>(4 * 1024 * 1024) {
        override fun sizeOf(key: Key, value: Entry) = value.weight
    }
    private val columns = object : android.util.LruCache<ColumnKey, ColumnEntry>(4 * 1024 * 1024) {
        override fun sizeOf(key: ColumnKey, value: ColumnEntry) = value.weight
    }

    fun prepare(layout: JSONObject, scale: Float, payload: IntArray, owner: Long): PanelVectorGeometry {
        val next = mutableMapOf<Int, Entry>()
        val groups = layout.array("groups").objects().associateBy { it.getInt("id") }
        var cursor = 0
        while (cursor < payload.size) {
            check(cursor + 5 <= payload.size)
            val id = payload[cursor++]
            val generation = (payload[cursor++].toLong() and 0xffffffffL) or (payload[cursor++].toLong() shl 32)
            val commands = payload[cursor++]
            val triangles = payload[cursor++]
            check(commands >= 0 && triangles >= 0 && commands <= (payload.size - cursor) / 7)
            val meshStart = cursor + commands * 7
            check(triangles <= (payload.size - meshStart) / 7)
            val end = meshStart + triangles * 7
            val key = Key(owner, id, generation, scale)
            next[id] = paths[key] ?: run {
                val path = android.graphics.Path()
                val body = android.graphics.Path()
                val outputs = arrayOf(path,body)
                fun coordinate(index: Int) = payload[index] * (scale / 1024f)
                for (i in cursor until meshStart step 7) {
                    if (payload[i] == 0) body.rewind()
                    for (output in outputs) when (payload[i]) {
                        0 -> output.moveTo(coordinate(i + 1), coordinate(i + 2))
                        1 -> output.lineTo(coordinate(i + 1), coordinate(i + 2))
                        2 -> output.cubicTo(coordinate(i + 1), coordinate(i + 2), coordinate(i + 3), coordinate(i + 4), coordinate(i + 5), coordinate(i + 6))
                        3 -> output.close()
                    }
                }
                val outline = Outline.Generic(path.asComposePath())
                val mesh = (meshStart until end step 7).map { offset ->
                    FloatArray(7) { i -> payload[offset + i] * if (i == 6) 1f else scale / 1024f }
                }
                Entry(key, PanelVectorShape(outline, mesh, Outline.Generic(body.asComposePath())), 128 + commands * 128 + triangles * 48)
                    .also { paths.put(key, it) }
            }
            cursor = end
        }
        check(next.keys == layout.array("vector_surfaces").objects().map { it.getInt("group") }.toSet())
        val surfaces = next.mapValues { it.value.shape }
        val currentColumns = mutableListOf<PanelVectorColumn>()
        for (column in layout.array("collapsed").objects()) {
            val id = column.getInt("id")
            val members = layout.array("vector_surfaces").objects().filter { it.getInt("column") == column.getInt("id") }
            if (members.isEmpty()) continue
            val origin = column.getJSONObject("bounds")
            val sourceBounds=origin.rect()
            val panelBounds=column.getJSONObject("open").getJSONObject("bounds").rect()
            val global=androidx.compose.ui.geometry.Rect(
                minOf(sourceBounds.left,panelBounds.left)*scale,minOf(sourceBounds.top,panelBounds.top)*scale,
                maxOf(sourceBounds.right,panelBounds.right)*scale,maxOf(sourceBounds.bottom,panelBounds.bottom)*scale)
            val placed = members.map { member ->
                val group = member.getInt("group"); val b = groups.getValue(group).getJSONObject("bounds")
                PlacedShape(group, next.getValue(group).key.generation, b.number("x"), b.number("y"))
            }
            val key = ColumnKey(id, owner, listOf("x", "y", "width", "height").map { origin.number(it) } +
                (0..3).map { column.array("corner_radii").optDouble(it).toFloat() }, placed, scale, global)
            val retained = columns[key]
            if (retained != null) { currentColumns.add(retained.column); continue }
            val source = column.panelCornerShape().createOutline(Size(origin.number("width") * scale, origin.number("height") * scale), LayoutDirection.Ltr, Density(scale))
            val path = android.graphics.Path().apply {
                val local = androidx.compose.ui.graphics.Path().apply { addOutline(source) }.asAndroidPath()
                addPath(local, origin.number("x") * scale, origin.number("y") * scale)
            }
            for (member in placed) {
                val outline = surfaces.getValue(member.group).createOutline(Size.Zero, LayoutDirection.Ltr, Density(scale)) as Outline.Generic
                val piece = android.graphics.Path().apply { addPath(outline.path.asAndroidPath(), member.x * scale, member.y * scale) }
                check(path.op(piece, android.graphics.Path.Op.UNION))
            }
            path.offset(-global.left, -global.top)
            val shape = PanelVectorShape(Outline.Generic(path.asComposePath()))
            val prepared = PanelVectorColumn(id, members.map { it.getInt("group") }.toSet(),
                obj("x" to global.left / scale, "y" to global.top / scale,
                    "width" to global.width / scale, "height" to global.height / scale), shape)
            columns.put(key, ColumnEntry(prepared, 512 + placed.sumOf { next.getValue(it.group).weight }))
            currentColumns.add(prepared)
        }
        return PanelVectorGeometry(surfaces, currentColumns)
    }
}

@Composable internal fun Modifier.vectorGlass(shape: PanelVectorShape?): Modifier {
    val host = LocalCanvasHost.current
    val opacity = LocalWorkspaceOpacity.current
    val key = remember { Any() }
    var origin by remember { mutableStateOf(Offset.Zero) }
    DisposableEffect(host,key) { onDispose { host.glassVector(key,null,Offset.Zero,0f) } }
    val currentShape by rememberUpdatedState(shape)
    LaunchedEffect(host,key,opacity) {
        snapshotFlow { Triple(currentShape,origin,opacity?.value ?: 1f) }.collect { (surface,point,strength) ->
            host.glassVector(key,surface,point,strength)
        }
    }
    return onGloballyPositioned { origin=it.positionInRoot()-host.surfaceOrigin }
}

internal fun JSONObject.panelCornerShape(): Shape {
    val r = array("corner_radii")
    return SquircleShape(androidx.compose.ui.unit.Dp(r.optDouble(0, SurfaceRadius.value.toDouble()).toFloat()),
        androidx.compose.ui.unit.Dp(r.optDouble(1, SurfaceRadius.value.toDouble()).toFloat()),
        androidx.compose.ui.unit.Dp(r.optDouble(2, SurfaceRadius.value.toDouble()).toFloat()),
        androidx.compose.ui.unit.Dp(r.optDouble(3, SurfaceRadius.value.toDouble()).toFloat()), physical = true)
}
