package art.capycanvas

import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

class AndroidSavedStateTest {
    @get:Rule val device = CapyDeviceRule()
    @Test fun damagedWorkspaceStoreIsKeptAndFreshSettingsSurviveRestart() {
        val context = instrumentation.targetContext
        val storage = AppStorage.of(context)
        val bytes = "damaged SQLite store".toByteArray()
        val theme = androidx.test.platform.app.InstrumentationRegistry.getArguments().getString("theme") ?: "dark"
        assertTrue(context.getSharedPreferences(CanvasHost.preferencesName,0).edit().putString("settings",obj("theme" to theme,"language" to obj("Explicit" to "en")).toString()).commit())
        storage.workspaces.mkdirs()
        File(storage.workspaces,"workspaces.sqlite3").writeBytes(bytes)
        wakeDevice()
        androidx.test.core.app.ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            val host = scenario.activity().host
            host.awaitMain("damaged store adopted",120_000) {host.workspaceManager?.optBoolean("ready")==true && host.snapshot?.getJSONObject("state")?.isNull("notice")==false}
            val notice = host.snapshot!!.getJSONObject("state").getJSONObject("notice")
            assertEquals("Your saved workspaces were damaged. Capy Canvas started new ones.",notice.getString("text"))
            host.awaitMain("damaged notice rendered") {findNode(hasLabel(notice.getString("text")))!=null}
            val committed = CountDownLatch(1)
            instrumentation.runOnMainSync {
                val view = findTag("canvas-notice-text")!!.first.view
                view.viewTreeObserver.registerFrameCommitCallback {committed.countDown()}
                view.invalidate()
            }
            assertTrue("The damaged notice committed a frame",committed.await(5,TimeUnit.SECONDS))
            screenshot("validation/saved-data/damaged-$theme.png")
            val copies = storage.workspaces.listFiles()!!.filter {it.name.startsWith("workspaces.damaged.") && it.extension=="sqlite3"}
            assertEquals(1,copies.size)
            assertArrayEquals(bytes,copies.single().readBytes())
            host.awaitReady(120_000)
            host.drain(obj("type" to "set_brush_size","value" to 43.0))
            host.awaitMain("fresh settings saved",30_000) {host.workspaceManager?.let {!it.optBoolean("dirty") && !it.optBoolean("saving")}==true}
        }
        launchCapy(120_000).use { scenario ->
            val host = scenario.activity().host
            host.awaitMain("fresh store reopened",120_000) {host.surfaceReady}
            val state = host.snapshot!!.getJSONObject("state")
            assertTrue(state.isNull("notice"))
            assertEquals(43.0,state.getJSONObject("brush").getDouble("diameter"),0.0)
            assertArrayEquals(bytes,storage.workspaces.listFiles()!!.single {it.name.startsWith("workspaces.damaged.") && it.extension=="sqlite3"}.readBytes())
        }
    }
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
