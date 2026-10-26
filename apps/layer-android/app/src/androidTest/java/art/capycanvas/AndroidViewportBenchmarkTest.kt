package art.capycanvas

import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test

class AndroidViewportBenchmarkTest {
    @get:Rule val device = CapyDeviceRule(nativeFileJobs = true)

    @Test fun retainedViewport() {
        val args = InstrumentationRegistry.getArguments()
        assumeTrue(args.getString("viewportBenchmark") == "true")
        ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            ViewportFrameTiming(instrumentation, args).run(scenario.activity())
        }
    }
}
