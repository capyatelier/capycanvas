package art.capycanvas

import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.*
import org.json.JSONObject

@Composable internal fun DocumentPropertiesDialog(host: CanvasHost, onDismiss: () -> Unit, onAppearance: () -> Unit) {
    var view by remember { mutableStateOf<JSONObject?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(Unit) {
        try { withContext(NonCancellable) {
            val task = host.withNative { Native.documentInfoTask(it) }
            view = withContext(Dispatchers.IO) { JSONObject(Native.documentInfo(task)) }
        } } catch (e: Exception) { error = e.message }
    }
    val done=view?.getString("done") ?: host.bootstrap!!.getJSONObject("common").getString("done")
    val phone=host.snapshot?.getJSONObject("layout")?.objectOrNull("phone")!=null
    DialogShell(host,view?.getJSONObject("dialog") ?: host.bootstrap!!.getJSONObject("progress_dialog"),
        view?.getString("title") ?: host.bootstrap!!.getString("preparing_document"),onCancel=onDismiss,tag="document-properties",
        responses=(if(view!=null&&!phone)listOf(DialogResponse(view!!.getString("proof"),"document-properties-proof",action=onAppearance))else emptyList())+
            DialogResponse(done,"document-properties-done",action=onDismiss),
        secondary=if(view==null||!phone)null else ({TextButton(onAppearance){Text(view!!.getString("proof"))}}),body={padding ->
            Column(Modifier.fillMaxWidth().padding(padding), verticalArrangement = Arrangement.spacedBy(8.dp)) {
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
