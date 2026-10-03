package art.capycanvas

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import org.json.JSONArray
import org.json.JSONObject
import org.json.JSONTokener

@androidx.annotation.Keep
internal class ColorFeatureFailure : RuntimeException {
    val reason: Any
    constructor(reason: Any) : super() { this.reason = reason }
    constructor(encoded: String) : super(encoded) {
        val envelope = runCatching { JSONObject(encoded) }.getOrNull()
        reason = if (envelope?.length() == 1 && envelope.has("color_feature_error")) envelope.get("color_feature_error") else encoded
    }
}
internal fun JSONObject.shallowCopy(): JSONObject = JSONObject().also { copy -> keys().forEach { key -> copy.put(key,get(key)) } }
internal fun profileMetadata(entry: JSONObject): JSONObject = JSONObject().apply {
    for (key in listOf("id", "bytes", "name", "channels", "issue")) if (entry.has(key)) put(key, entry.get(key))
}
internal suspend fun profileEntriesCopy(host: CanvasHost, entries: List<JSONObject>): List<JSONObject> {
    val result = mutableListOf<JSONObject>()
    for (batch in entries.chunked(host.withNative { ProfileStore.entryLimit })) {
        val projected = JSONArray(host.withNative { Native.query(it, obj("type" to "profile_entries_copy", "entries" to JSONArray(batch.map(::profileMetadata))).toString()) })
        currentCoroutineContext().ensureActive()
        result += projected.objects().mapIndexed { index, entry -> entry.put("visible", batch[index].optBoolean("visible", true)) }
    }
    return result
}
@Composable internal fun profileCaption(host: CanvasHost, profile: JSONObject): String {
    var caption by remember(profile) { mutableStateOf(profile.optString("name")) }
    LaunchedEffect(profile, host.languageTag) {
        val language = host.languageTag
        val next = if (profile.has("id")) profileEntriesCopy(host, listOf(profile)).single().getString("name") else JSONTokener(host.withNative { Native.query(it, obj("type" to "export_profile_name_copy", "name" to profile.getString("name")).toString()) }).nextValue() as String
        currentCoroutineContext().ensureActive()
        if (language == host.languageTag) caption = next
    }
    return caption
}
internal suspend fun profileNameCopy(host: CanvasHost, name: String?): String = JSONTokener(host.withNative {
    Native.query(it, obj("type" to "profile_name_copy", "name" to name).toString())
}).nextValue() as String
internal suspend fun presetNamesCopy(host: CanvasHost, names: List<String>): List<String> = JSONArray(host.withNative {
    Native.query(it, obj("type" to "export_preset_copy", "names" to JSONArray(names)).toString())
}).values().map { it as String }
internal suspend fun colorFailureCopy(host: CanvasHost, error: Exception, profile: Boolean? = null, proof: Boolean = false): String {
    return if (error is ColorFeatureFailure) JSONTokener(host.withNative {
        Native.query(it, obj("type" to "color_feature_error_copy", "reason" to error.reason, "profile" to profile, "proof" to proof).toString())
    }).nextValue() as String else error.message ?: host.bootstrap!!.getString("action_failed")
}
@Composable internal fun ColorFailureText(host: CanvasHost, error: Exception?, profile: Boolean? = null, proof: Boolean = false) {
    var caption by remember(error) { mutableStateOf<String?>(null) }
    LaunchedEffect(error, profile, proof, host.languageTag) {
        val language = host.languageTag
        val next = error?.let { colorFailureCopy(host, it, profile, proof) }
        currentCoroutineContext().ensureActive()
        if (language == host.languageTag) caption = next
    }
    caption?.let { Text(it, color = MaterialTheme.colorScheme.error) }
}
