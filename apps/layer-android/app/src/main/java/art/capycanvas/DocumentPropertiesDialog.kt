package art.capycanvas

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.*
import org.json.JSONObject

@Composable internal fun DocumentPropertiesDialog(host: CanvasHost, onDismiss: () -> Unit) {
    var view by remember { mutableStateOf<JSONObject?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(Unit) {
        try { withContext(NonCancellable) {
            val task = host.withNative { Native.documentInfoTask(it) }
            view = withContext(Dispatchers.IO) { JSONObject(Native.documentInfo(task)) }
        } } catch (e: Exception) { error = e.message }
    }
    AlertDialog(onDismissRequest = onDismiss, title = { Text(view?.getString("title") ?: host.bootstrap?.getString("preparing_document").orEmpty()) },
        confirmButton = { TextButton(onDismiss) { Text(view?.getString("done") ?: host.bootstrap?.getJSONObject("common")?.getString("done").orEmpty()) } }, text = {
            Column(Modifier.fillMaxWidth().heightIn(max = 580.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                view?.getJSONArray("rows")?.let { values -> for (i in 0 until values.length()) {
                    val row = values.getJSONArray(i)
                    Text(row.getString(0), style = MaterialTheme.typography.titleSmall)
                    Text(row.getString(1))
                } }
                view?.let { properties ->
                    val sources = properties.getJSONArray("sources")
                    if (sources.length() > 0) Text(properties.getString("source_images"), style = MaterialTheme.typography.titleMedium)
                    for (i in 0 until sources.length()) {
                        val row = sources.getJSONArray(i)
                        Text(row.getString(0), style = MaterialTheme.typography.titleSmall)
                        Text(row.getString(1))
                    }
                }
                if (view == null && error == null) { CircularProgressIndicator() }
                error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            }
        })
}
