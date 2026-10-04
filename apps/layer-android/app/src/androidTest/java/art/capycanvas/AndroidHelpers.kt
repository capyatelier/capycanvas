package art.capycanvas

import org.json.JSONObject

internal fun snapshotSource(kind: String, occurrenceToken: Long): String {
    require(kind == "EffectInput" || kind == "EffectChannels")
    require(occurrenceToken in 1..0xffffffffL)
    return obj(kind to (occurrenceToken - 1)).toString()
}

internal fun snapshotSource(target: JSONObject): String {
    require(target.length() == 1)
    val kind = target.keys().next()
    require(kind in listOf("Paint", "Coverage", "Selection"))
    require(target.getLong(kind) in 0..0xfffffffeL)
    return obj("Source" to target).toString()
}
