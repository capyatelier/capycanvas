package art.capycanvas

import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test

class AndroidNativeHandlesTest {
    @get:Rule val device = CapyDeviceRule(nativeFileJobs = true)

    private fun refused(work: () -> Unit) {
        try { work(); fail("Expected an invalid native handle to be refused") }
        catch (error: IllegalStateException) { assertFalse(error.message.isNullOrEmpty()) }
    }

    @Test fun staleAndWrongKindsNeverAccessAnotherAllocation() {
        for (invalid in listOf(1L, -1L, Long.MAX_VALUE)) {
            refused { Native.captureCancel(invalid) }
            refused { Native.captureFree(invalid) }
            refused { Native.snapshot(invalid) }
        }
        val first = Native.captureControl()
        assertTrue(first > 0)
        refused { Native.snapshot(0) }
        refused { Native.destroy(first) }
        refused { Native.freeLanguage(first) }
        refused { Native.colorFree(first) }
        refused { Native.sourceFree(first) }
        refused { Native.imageImportFree(first) }
        refused { Native.proofRelease(first) }
        refused { Native.sessionFree(first) }
        refused { Native.sessionStoresFree(first) }
        refused { Native.documentResumeFree(first) }
        refused { Native.clipFree(first) }
        refused { Native.clipTaskFree(first) }
        refused { Native.projectFree(first) }
        refused { Native.paperFree(first) }
        refused { Native.projectPackagePrompt(first) }
        Native.captureCancel(first)
        Native.captureFree(first)
        refused { Native.captureCancel(first) }
        refused { Native.captureFree(first) }
        val replacement = Native.captureControl()
        try {
            assertNotEquals(first, replacement)
            refused { Native.captureFree(first) }
            Native.captureCancel(replacement)
        } finally { Native.captureFree(replacement) }
        val language = Native.prepareLanguage("en")
        try {
            refused { Native.captureCancel(language) }
            refused { Native.captureFree(language) }
        } finally { Native.freeLanguage(language) }
        refused { Native.freeLanguage(language) }
    }

    @Test fun appAccessStaysOnItsOwnerAndTerminalJobsConsumeOnce() {
        val owner = Executors.newSingleThreadExecutor()
        fun <T> onOwner(work: () -> T): T = owner.submit<T> { work() }.get(30, TimeUnit.SECONDS)
        var app = 0L
        try {
            app = onOwner { Native.create("", arrayOf("en-US"), "", false) }
            assertTrue(app > 0)
            refused { Native.snapshot(app) }
            refused { Native.destroy(app) }
            val info = onOwner {
                assertNotNull(Native.snapshot(app))
                Native.documentInfoTask(app)
            }
            refused { Native.captureFree(info) }
            assertTrue(Native.documentInfo(info).isNotEmpty())
            refused { Native.documentInfo(info) }
            onOwner { Native.destroy(app) }
            val retired = app; app = 0L
            onOwner { refused { Native.snapshot(retired) }; refused { Native.destroy(retired) } }
        } finally {
            if (app != 0L) onOwner { Native.destroy(app) }
            owner.shutdown()
            assertTrue(owner.awaitTermination(30, TimeUnit.SECONDS))
        }
    }
}
