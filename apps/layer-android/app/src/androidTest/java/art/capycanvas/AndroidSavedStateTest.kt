package art.capycanvas

import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import java.io.File

class AndroidSavedStateTest {
    @get:Rule val device = CapyDeviceRule()
    @Test fun releasedStoreKeepsEditingPreferencesAndPresetsAfterRestart() {
        val context = instrumentation.targetContext
        val storage = AppStorage.of(context)
        fun copy(name: String, destination: File) {
            destination.parentFile!!.mkdirs()
            instrumentation.context.assets.open(name).use { input -> destination.outputStream().use { input.copyTo(it) } }
        }
        copy("workspaces.sqlite3", File(storage.workspaces,"workspaces.sqlite3"))
        copy("export-presets",storage.exportPresets)
        val settings = instrumentation.context.assets.open("settings.json").bufferedReader().use { it.readText() }
        assertTrue(context.getSharedPreferences(CanvasHost.preferencesName,0).edit().putString("settings",settings).commit())
        wakeDevice()
        var diameter = 37.0
        for (theme in listOf("light","dark")) {
            launchCapy(120_000).use { scenario ->
                val host = scenario.activity().host
                host.drain(obj("type" to "set_theme","theme" to theme))
                host.awaitMain("first restored canvas",120_000) { host.surfaceReady }
                val state = host.snapshot!!.getJSONObject("state")
                assertEquals(diameter,state.getJSONObject("brush").getDouble("diameter"),0.0)
                assertEquals(1.5,state.getJSONObject("settings").getDouble("pan_speed"),0.0)
                assertTrue(state.isNull("notice"))
                assertTrue(state.getJSONObject("color_library").getJSONArray("palettes").objects().any { palette ->
                    palette.getString("name")=="Release palette" && palette.getJSONArray("swatches").objects().any { it.getString("name")=="Release blue" }
                })
                assertEquals(BuildConfig.VERSION_NAME,File(storage.workspaces,"last-version").readText())
                assertEquals(1,storage.workspaces.listFiles()!!.count { it.name.startsWith("workspaces.earlier.") && it.extension=="sqlite3" })
                assertTrue(host.workspaceManager!!.isNull("error"))
                val presets = runBlocking { ColorPreferencesStore.presets(context,obj("space" to "Srgb","depth" to "U8"),obj("type" to "list")) }
                assertTrue(presets.getJSONArray("names").values().contains("Release delivery"))
                diameter = 43.0
                host.drain(obj("type" to "set_brush_size","value" to diameter))
                host.awaitMain("editing save",30_000) { host.workspaceManager?.let { !it.optBoolean("dirty") && !it.optBoolean("saving") } == true }
            }
        }
    }
}
