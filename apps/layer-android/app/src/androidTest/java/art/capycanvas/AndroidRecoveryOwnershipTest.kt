package art.capycanvas

import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import java.nio.channels.FileChannel
import java.nio.file.StandardOpenOption
import java.util.UUID
import java.util.concurrent.TimeUnit
import org.junit.Assert.*
import org.junit.Test

class AndroidRecoveryOwnershipTest {
    private fun probe(file: File, expected: Int) {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val process = ProcessBuilder("/system/bin/app_process", "/system/bin", "art.capycanvas.SessionOwnerLockProbe", file.absolutePath)
            .redirectErrorStream(true).apply {
                environment()["CLASSPATH"] = instrumentation.context.packageCodePath + ":" + instrumentation.targetContext.packageCodePath
            }.start()
        if (!process.waitFor(30, TimeUnit.SECONDS)) { process.destroyForcibly(); fail("Session lock probe did not exit") }
        assertEquals(process.inputStream.bufferedReader().readText(), expected, process.exitValue())
    }
    @Test fun liveJavaOwnerSurvivesSharedImportAndStillExcludesAnotherProcess() {
        val root = File(InstrumentationRegistry.getInstrumentation().targetContext.cacheDir, "session-owner-${UUID.randomUUID()}")
        val source = File(root, "window")
        val owner = checkNotNull(SessionOwners.claim(source))
        try {
            val bytes = "{\"generation\":1,\"drawings\":[],\"active\":0,\"clean_exit\":false,\"restoring\":[],\"blocked\":[]}"
            File(source, "session.json").writeText(bytes)
            val packed = File(Native.sessionImport(root.absolutePath))
            assertEquals(bytes, File(source, "session.json").readText())
            assertFalse(File(packed, "window").exists())
            assertTrue(owner.lock.isValid)
            probe(File(source, "owner.lock"), 0)
        } finally { owner.close() }
        try {
            val packed = File(Native.sessionImport(root.absolutePath))
            assertFalse(source.exists()); assertTrue(File(packed, "window/session.json").isFile)
            probe(File(packed, "window/owner.lock"), 1)
        } finally { root.deleteRecursively() }
    }
}

object SessionOwnerLockProbe {
    @JvmStatic fun main(arguments: Array<String>) {
        FileChannel.open(File(arguments[0]).toPath(), StandardOpenOption.WRITE).use { channel ->
            val lock = channel.tryLock()
            if (lock == null) kotlin.system.exitProcess(0)
            lock.release(); kotlin.system.exitProcess(1)
        }
    }
}
