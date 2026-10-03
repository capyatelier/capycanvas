package art.capycanvas

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject

@Composable internal fun ProfileFileButton(host:CanvasHost,label:String?=null,onProfile: (JSONObject) -> Unit) {
    val copy = host.catalog.getJSONObject("profile_copy")
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var error by remember { mutableStateOf<Exception?>(null) }
    var busy by remember { mutableStateOf(false) }
    val picker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) scope.launch {
            busy = true
            try {
                val profile = withContext(Dispatchers.IO) {
                    context.contentResolver.openInputStream(uri)?.use { input ->
                        val bytes = ByteArray(copy.getInt("read_bytes") + 1); var length = 0
                        while (length < bytes.size) { val n = input.read(bytes, length, bytes.size-length); if (n < 0) break; length += n }
                        if (length == bytes.size) throw ColorFeatureFailure("ProfileReadLimit")
                        ProfileStore.import(context,bytes.copyOf(length))
                    } ?: error(host.bootstrap!!.getString("action_failed"))
                }
                onProfile(profile); error = null
            } catch (e: Exception) { error = e }
            finally { busy = false }
        }
    }
    Column {
        TextButton(enabled = !busy, onClick = { picker.launch(arrayOf("*/*")) }) { Text(if (busy) copy.getString("reading") else label ?: copy.getString("import")) }
        ColorFailureText(host, error, true)
    }
}

@Composable internal fun SourceProfileDialog(host:CanvasHost,interpretation: JSONObject, onChoose: (JSONObject?) -> Unit) {
    val copy = host.catalog.getJSONObject("profile_copy")
    var selection by remember { mutableStateOf("Srgb") }
    var custom by remember { mutableStateOf<JSONObject?>(null) }
    AlertDialog(onDismissRequest = { onChoose(null) }, title = { Text(copy.getString("interpret_title")) },
        text = { Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(copy.getString("interpret_help"))
            ColorChoice(copy.getString("interpret_as"), listOf("Srgb" to "sRGB", "DisplayP3" to "Display P3", "AdobeRgb" to "Adobe RGB (1998)", "ProPhoto" to "ProPhoto RGB") +
                (custom?.let { listOf("custom" to profileCaption(host, it)) } ?: emptyList()), selection) { selection = it }
            ImportProfileButton(host) { custom = it; selection = "custom" }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(interpretation.getString("channels"))
                Text(host.catalog.getJSONObject("export_copy").getString(if (interpretation.getString("depth") == "U16") "depth_16" else "depth_8"))
            }
        } }, dismissButton = { TextButton({ onChoose(null) }) { Text(copy.getJSONObject("common").getString("cancel")) } },
        confirmButton = { TextButton({ onChoose(if(selection=="custom")custom!!.getJSONObject("profile") else obj("Builtin" to selection)) }) { Text(copy.getString("use_profile")) } })
}

@Composable internal fun ImportProfileButton(host:CanvasHost,onProfile:(JSONObject)->Unit) {
    val copy = host.catalog.getJSONObject("profile_copy")
    var library by remember {mutableStateOf(false)}
    Row {ProfileFileButton(host,onProfile=onProfile);TextButton({library=true}){Text(copy.getString("saved_dialog"))}}
    if(library)ProfileLibraryDialog(host,{library=false}){profile->onProfile(profile);library=false}
}

@Composable internal fun ProfileLibraryDialog(host:CanvasHost,onDismiss:()->Unit,onProfile:((JSONObject)->Unit)?=null) {
    val copy = host.catalog.getJSONObject("profile_copy")
    val context=LocalContext.current
    val scope=rememberCoroutineScope()
    var entries by remember {mutableStateOf<List<JSONObject>>(emptyList())}
    var rawEntries by remember {mutableStateOf<List<JSONObject>>(emptyList())}
    var error by remember {mutableStateOf<Exception?>(null)}
    var busy by remember {mutableStateOf(true)}
    var refresh by remember {mutableIntStateOf(0)}
    LaunchedEffect(refresh){busy=true;try{rawEntries=ProfileStore.list(context);error=null}catch(e:Exception){error=e}finally{busy=false}}
    LaunchedEffect(rawEntries, host.languageTag) {
        val language=host.languageTag
        val retained=rawEntries
        val projected=profileEntriesCopy(host,retained)
        if(language==host.languageTag && retained===rawEntries)entries=projected
    }
    AlertDialog(onDismissRequest=onDismiss,title={Text(copy.getString("library_title"))},confirmButton={TextButton(onDismiss,Modifier.testTag("profile-library-done")){Text(copy.getJSONObject("common").getString("done"))}},text={
        Column(Modifier.fillMaxWidth().heightIn(max=580.dp).verticalScroll(rememberScrollState()),verticalArrangement=Arrangement.spacedBy(8.dp)){
            Text(copy.getString("library_help"))
            if(busy)CircularProgressIndicator()
            for(entry in entries){
                Text(entry.getString("name"),style=MaterialTheme.typography.titleSmall)
                Text(entry.optString("issue").ifEmpty { entry.getString("details") })
                Row {
                    if(onProfile!=null)TextButton(enabled=!busy&&!entry.has("issue"),onClick={busy=true;scope.launch{try{onProfile(ProfileStore.get(context,entry.getString("id")))}catch(e:Exception){error=e}finally{busy=false}}}){Text(copy.getString("use_profile"))}
                    TextButton(enabled=!busy,onClick={busy=true;scope.launch{try{ProfileStore.remove(context,entry.getString("id"));refresh++}catch(e:Exception){error=e}finally{busy=false}}}){Text(copy.getString("remove"))}
                }
                TextButton(enabled=!busy,onClick={busy=true;scope.launch{try{ProfileStore.show(context,entry.getString("id"),!entry.optBoolean("visible",true));refresh++}catch(e:Exception){error=e}finally{busy=false}}}){Text(if(entry.optBoolean("visible",true))copy.getString("hide") else copy.getString("show"))}
            }
            if(entries.isEmpty()&&!busy)Text(copy.getString("empty"))
            ProfileFileButton(host) {if(onProfile!=null)onProfile(it)else refresh++}
            ColorFailureText(host, error, true)
        }
    })
}
