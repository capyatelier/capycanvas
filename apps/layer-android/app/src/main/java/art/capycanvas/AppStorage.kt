package art.capycanvas

import android.content.Context
import android.util.Log
import org.json.JSONObject
import java.io.File

/** The platform folder for each kind of file. Shared Rust names the stores within them. */
internal class AppStorage private constructor(stores: JSONObject) {
    val workspaces = File(stores.getString("workspaces"))
    val sessions = File(stores.getString("sessions"))
    val shaders = File(stores.getString("shaders"))
    val clipboard = File(stores.getString("clipboard"))
    val colorProfiles = File(stores.getString("color_profiles"))
    val exportPresets = File(stores.getString("export_presets"))
    private val temp = File(stores.getString("temp"))

    fun temporaryFile(prefix: String, suffix: String): File = File.createTempFile(prefix, suffix, temp.apply { mkdirs() })

    companion object {
        /** Replaces the folders for settings, user data and session state. */
        @Volatile internal var directoryForTest: File? = null
        private var cleared = false

        /** The first call in a process removes the files an earlier process left in the temporary folder. */
        @Synchronized fun of(context: Context): AppStorage {
            val isolated = directoryForTest
            val storage = AppStorage(JSONObject(Native.storage(
                (isolated?.resolve("config") ?: context.filesDir).absolutePath,
                (isolated?.resolve("data") ?: context.filesDir).absolutePath,
                (isolated?.resolve("state") ?: context.noBackupFilesDir).absolutePath,
                context.cacheDir.absolutePath,
                File(context.cacheDir, "temp").absolutePath)))
            if (!cleared) {
                cleared = true
                if (!storage.temp.deleteRecursively()) Log.w("CapyCanvas", "Could not clear temporary files")
            }
            return storage
        }
    }
}
