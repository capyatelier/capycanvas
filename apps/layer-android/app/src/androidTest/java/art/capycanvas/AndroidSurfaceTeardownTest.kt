package art.capycanvas

import android.os.SystemClock
import androidx.lifecycle.Lifecycle
import androidx.test.core.app.ActivityScenario
import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

class AndroidSurfaceTeardownTest {
    @get:Rule val device = CapyDeviceRule()

    @Test fun backgroundingDuringALongWorkerTaskKeepsTheCanvas() {
        ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            lateinit var host: CanvasHost
            scenario.onActivity { host = it.host }
            fun waitForSurface(description: String) {
                val deadline = SystemClock.uptimeMillis() + 60_000
                var ready = false
                while (!ready && SystemClock.uptimeMillis() < deadline) {
                    instrumentation.runOnMainSync { ready = host.surfaceReady }
                    if (!ready) SystemClock.sleep(50)
                }
                assertTrue(description, ready)
            }
            waitForSurface("The canvas presents before backgrounding")
            val started = CountDownLatch(1)
            val busy = Thread { runBlocking { host.withNative { started.countDown(); SystemClock.sleep(12_000) } } }
            busy.start()
            assertTrue(started.await(10, TimeUnit.SECONDS))
            scenario.moveToState(Lifecycle.State.CREATED)
            scenario.moveToState(Lifecycle.State.RESUMED)
            busy.join(30_000)
            assertFalse(busy.isAlive)
            waitForSurface("The canvas presents again after the long task")
            instrumentation.runOnMainSync { assertNull(host.failure) }
        }
    }
}
