package art.capycanvas

import org.json.JSONArray
import org.json.JSONObject
import java.io.ByteArrayInputStream
import java.util.zip.ZipInputStream

internal fun packageMember(bytes: ByteArray, name: String): ByteArray = ZipInputStream(ByteArrayInputStream(bytes)).use { zip ->
    while (true) {
        val entry = zip.nextEntry ?: error("Missing package member $name")
        if (entry.name == name) return@use zip.readBytes()
    }
    error("Missing package member $name")
}

internal fun packageManifest(bytes: ByteArray): JSONObject {
    check(packageMember(bytes, "mimetype").decodeToString() == "application/vnd.capycanvas")
    return JSONObject(packageMember(bytes, "manifest.json").decodeToString()).also {
        check(it.getString("format") == "capy.canvas")
        check(it.getInt("version") == 1)
    }
}
internal fun JSONObject.packageRecord(id: String): JSONObject = getJSONArray("objects").objects().single { it.getString("id") == id }
internal fun JSONObject.packageData(id: String): JSONObject = packageRecord(id).getJSONObject("data")
internal fun JSONObject.compositionData(): JSONObject = packageData(getJSONObject("root").getString("ref"))
internal fun JSONObject.outputData(): JSONObject = packageData(getJSONObject("default_output").getString("ref"))
internal fun JSONObject.compositionColor(): JSONObject = compositionData().optJSONObject("color") ?: JSONObject()
internal fun JSONObject.compositionSize(): JSONArray = compositionData().getJSONArray("size")
internal fun JSONObject.occurrenceRecords(): JSONArray {
    val result = JSONArray()
    fun stack(id: String) {
        for (entry in packageData(id).optJSONArray("entries")?.objects().orEmpty()) {
            val record = packageRecord(entry.getString("ref")); result.put(record)
            record.getJSONObject("data").getJSONObject("content").optJSONObject("stack")?.let { stack(it.getString("ref")) }
        }
    }
    stack(compositionData().getJSONObject("result").getJSONObject("object").getString("ref"))
    return result
}
internal fun JSONObject.paintData(occurrence: String): JSONObject = packageData(packageData(occurrence).getJSONObject("content").getJSONObject("paint").getString("ref"))
internal fun JSONObject.artworkRecords(): JSONArray = JSONArray(getJSONArray("objects").objects().filter {it.getString("type") !in listOf("capy.composition/2","capy.output/2")})
internal fun JSONObject.paintRecords(): JSONArray = JSONArray(getJSONArray("objects").objects().filter { it.getString("type") == "capy.paint-source/2" })
internal fun JSONObject.paintBaseBindings(): JSONArray = JSONArray(occurrenceRecords().objects().mapNotNull {
    val source = it.getJSONObject("data").getJSONObject("content").optJSONObject("paint") ?: return@mapNotNull null
    packageData(source.getString("ref")).optJSONObject("base")
})
internal fun JSONObject.originalImages(): JSONArray = JSONArray(paintBaseBindings().objects().map { packageData(it.getJSONObject("image").getString("ref")) })
internal fun JSONObject.resourcesOf(type: String): JSONArray = JSONArray(getJSONArray("resources").objects().filter { it.getString("type") == type })
internal fun JSONObject.rasterResources(): JSONArray = JSONArray(resourcesOf("capy.raster-tile/1").objects().map { JSONObject(it.toString()).apply { remove("location") } })
internal fun JSONObject.profileIdentity(): String = resourcesOf("capy.icc/1").objects().map {
    JSONObject(it.toString()).apply { remove("id"); remove("location") }.toString()
}.sorted().toString()
internal fun JSONObject.profileRecordsById(): Map<String, String> = resourcesOf("capy.icc/1").objects().associate {
    it.getString("id") to JSONObject(it.toString()).apply { remove("location") }.toString()
}
internal fun JSONObject.profileContentIdentity(): Set<String> = resourcesOf("capy.icc/1").objects().map {
    JSONObject(it.toString()).apply { remove("id"); remove("location") }.toString()
}.toSet()
internal fun JSONObject.resourceData(reference: JSONObject): JSONObject = getJSONArray("resources").objects().single { it.getString("id") == reference.getString("ref") }
internal fun JSONObject.resolvedResourceBindings(value: Any): Any = when (value) {
    is JSONObject -> if (value.length() == 1 && value.has("ref")) {
        resolvedResourceBindings(JSONObject(resourceData(value).toString()).apply { remove("id"); remove("location") })
    } else JSONObject().apply { for (key in value.keys().asSequence().toList().sorted()) put(key, this@resolvedResourceBindings.resolvedResourceBindings(value.get(key))) }
    is JSONArray -> JSONArray((0 until value.length()).map { resolvedResourceBindings(value.get(it)) })
    else -> value
}
internal fun JSONObject.originalTileIdentity(image: JSONObject): String = resolvedResourceBindings(image.getJSONArray("tiles")).toString()
internal fun JSONObject.originalIdentity(): String = originalImages().objects().map { resolvedResourceBindings(it).toString() }.sorted().toString()
internal fun JSONObject.imageObjectRecords(): List<JSONObject> = getJSONArray("objects").objects().filter { it.getString("type") == "capy.image-object/1" }
internal fun JSONObject.objectImages(): JSONArray = JSONArray(imageObjectRecords().map { packageData(it.getJSONObject("data").getJSONObject("image").getString("ref")) })
internal fun JSONObject.objectImageIdentity(): String = objectImages().objects().map { resolvedResourceBindings(it).toString() }.sorted().toString()
