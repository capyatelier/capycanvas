package art.capycanvas

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject

@Composable internal fun ImportProfileButton(onProfile: (JSONObject) -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    val picker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) scope.launch {
            busy = true
            try {
                val profile = withContext(Dispatchers.IO) {
                    context.contentResolver.openInputStream(uri)?.use { input ->
                        val bytes = ByteArray(16 * 1024 * 1024 + 1); var length = 0
                        while (length < bytes.size) { val n = input.read(bytes, length, bytes.size-length); if (n < 0) break; length += n }
                        check(length < bytes.size) { "ICC profile exceeds 16 MiB" }
                        JSONObject(Native.inspectProfile(bytes.copyOf(length)))
                    } ?: error("Could not read the selected profile")
                }
                onProfile(profile); error = null
            } catch (e: Exception) { error = e.message ?: "Could not import the ICC profile" }
            finally { busy = false }
        }
    }
    Column {
        TextButton(enabled = !busy, onClick = { picker.launch(arrayOf("*/*")) }) { Text(if (busy) "Reading profile…" else "Import ICC Profile…") }
        error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
    }
}

@Composable internal fun SourceProfileDialog(interpretation: JSONObject, onChoose: (JSONObject?) -> Unit) {
    var selection by remember { mutableStateOf("Srgb") }
    var custom by remember { mutableStateOf<JSONObject?>(null) }
    AlertDialog(onDismissRequest = { onChoose(null) }, title = { Text("Choose image interpretation") },
        text = { Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text("This image has no declared color profile. Choose how to interpret its stored values. The original numbers will be retained.")
            ColorChoice("Interpret as", listOf("Srgb" to "sRGB", "DisplayP3" to "Display P3", "AdobeRgb" to "Adobe RGB (1998)", "ProPhoto" to "ProPhoto RGB") +
                (custom?.let { listOf("custom" to it.getString("name")) } ?: emptyList()), selection) { selection = it }
            ImportProfileButton { custom = it; selection = "custom" }
            Text("Source: ${interpretation.getString("channels")} · ${if(interpretation.getString("depth")=="U16")16 else 8}-bit")
        } }, dismissButton = { TextButton({ onChoose(null) }) { Text("Cancel") } },
        confirmButton = { TextButton({ onChoose(if(selection=="custom")custom!!.getJSONObject("profile") else obj("Builtin" to selection)) }) { Text("Use Profile") } })
}
