package art.capycanvas

import android.content.ComponentName
import android.content.Context
import android.content.pm.PackageManager
import android.util.Log
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

internal class HomeScreenIcon(private val context: Context, private val scope: CoroutineScope) {
    private val switching = Mutex()
    @Volatile private var wanted: String? = null

    fun show(icon: String) {
        wanted = icon
        scope.launch(Dispatchers.IO) { switching.withLock { wanted?.let(::switch) } }
    }

    private fun switch(icon: String) = runCatching {
        val packages = context.packageManager
        val aliases = context.resources.getStringArray(R.array.capy_launch_icons).associateWith { launcher(context, it) }
        if (icon !in aliases) return@runCatching
        fun shown(alias: ComponentName) = when (packages.getComponentEnabledSetting(alias)) {
            PackageManager.COMPONENT_ENABLED_STATE_ENABLED -> true
            PackageManager.COMPONENT_ENABLED_STATE_DEFAULT -> packages.getActivityInfo(alias, PackageManager.MATCH_DISABLED_COMPONENTS).enabled
            else -> false
        }
        for ((name, alias) in aliases.entries.sortedByDescending { it.key == icon }) {
            if (shown(alias) == (name == icon)) continue
            packages.setComponentEnabledSetting(alias, if (name == icon) PackageManager.COMPONENT_ENABLED_STATE_ENABLED
                else PackageManager.COMPONENT_ENABLED_STATE_DISABLED, PackageManager.DONT_KILL_APP)
        }
    }.onFailure { Log.e("CapyCanvas", "Could not show the $icon home screen icon", it) }

    companion object {
        fun launcher(context: Context, icon: String) = ComponentName(context,
            "art.capycanvas.Launcher" + icon.split('-').joinToString("") { it.replaceFirstChar(Char::uppercaseChar) })
    }
}
