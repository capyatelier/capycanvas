package art.capycanvas

import android.app.ActivityManager
import android.app.Application
import android.content.ComponentCallbacks2
import android.content.res.Configuration
import android.os.Looper
import java.util.concurrent.ConcurrentLinkedQueue

internal object SystemMemory : ComponentCallbacks2 {
    @Volatile private var activities: ActivityManager? = null
    private val trims = ConcurrentLinkedQueue<Long>()

    fun attach(application: Application) {
        if (activities != null) return
        application.registerComponentCallbacks(this)
        activities = application.getSystemService(ActivityManager::class.java)
    }

    @JvmStatic fun sample(): LongArray? {
        val activities = activities ?: return null
        if (Looper.getMainLooper().isCurrentThread) return null
        val info = ActivityManager.MemoryInfo().also(activities::getMemoryInfo)
        val levels = generateSequence { trims.poll() }.toList().toLongArray()
        return longArrayOf(info.totalMem, info.availMem, info.threshold, if (info.lowMemory) 1 else 0) + levels
    }

    override fun onTrimMemory(level: Int) { trims.add(level.toLong()) }
    @Deprecated("Deprecated in Java") @Suppress("DEPRECATION") override fun onLowMemory() { trims.add(ComponentCallbacks2.TRIM_MEMORY_COMPLETE.toLong()) }
    override fun onConfigurationChanged(configuration: Configuration) = Unit
}
