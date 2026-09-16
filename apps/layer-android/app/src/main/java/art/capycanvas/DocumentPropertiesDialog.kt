package art.capycanvas

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.*
import org.json.JSONArray

@Composable internal fun DocumentPropertiesDialog(host: CanvasHost, onDismiss: () -> Unit) {
    var rows by remember { mutableStateOf<JSONArray?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(Unit) {
        try { withContext(NonCancellable) {
            val task = host.withNative { Native.documentInfoTask(it) }
            rows = withContext(Dispatchers.IO) { JSONArray(Native.documentInfo(task)) }
        } } catch (e: Exception) { error = e.message }
    }
    AlertDialog(onDismissRequest = onDismiss, title = { Text("Document Properties") },
        confirmButton = { TextButton(onDismiss) { Text("Done") } }, text = {
            Column(Modifier.fillMaxWidth().heightIn(max = 580.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                rows?.let { values -> for (i in 0 until values.length()) {
                    val row = values.getJSONArray(i)
                    Text(row.getString(0), style = MaterialTheme.typography.titleSmall)
                    Text(row.getString(1))
                } }
                if (rows == null && error == null) { CircularProgressIndicator() }
                error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            }
        })
}
