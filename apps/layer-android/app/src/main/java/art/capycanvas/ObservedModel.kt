package art.capycanvas

import androidx.compose.runtime.MutableIntState
import androidx.compose.runtime.mutableIntStateOf
import org.json.JSONArray
import org.json.JSONObject

internal class ObservedModel(private val nested: List<String> = emptyList()) : JSONObject() {
    private val versions = HashMap<String, MutableIntState>()
    private val shape = mutableIntStateOf(0)

    private fun observe(name: String?) {
        val version = name?.let { versions[it] }
        if (version != null) version.intValue else shape.intValue
    }

    override fun opt(name: String?): Any? { observe(name); return super.opt(name) }
    override fun get(name: String): Any { observe(name); return super.get(name) }
    override fun has(name: String?): Boolean { observe(name); return super.has(name) }
    override fun isNull(name: String?): Boolean { observe(name); return super.isNull(name) }
    override fun length(): Int { shape.intValue; return super.length() }
    override fun keys(): MutableIterator<String> { shape.intValue; return super.keys() }
    override fun names(): JSONArray? { shape.intValue; return super.names() }

    override fun put(name: String, value: Any?): JSONObject = apply { store(name, value) }
    override fun put(name: String, value: Boolean): JSONObject = put(name, value as Any?)
    override fun put(name: String, value: Double): JSONObject = put(name, value as Any?)
    override fun put(name: String, value: Int): JSONObject = put(name, value as Any?)
    override fun put(name: String, value: Long): JSONObject = put(name, value as Any?)
    override fun remove(name: String?): Any? {
        val value = name?.let { super.opt(it) }
        if (name != null) store(name, null)
        return value
    }

    fun assign(next: JSONObject) {
        for (name in next.keys()) {
            val value = next.opt(name)
            if (value is JSONObject && nested.any { it == name || it.startsWith("$name.") }) child(name).assign(value) else store(name, value)
        }
        super.keys().asSequence().filterNot(next::has).toList().forEach { store(it, null) }
    }

    private fun child(name: String) = super.opt(name) as? ObservedModel
        ?: ObservedModel(nested.filter { it.startsWith("$name.") }.map { it.removePrefix("$name.") }).also { store(name, it) }

    private fun store(name: String, value: Any?) {
        val previous = super.opt(name)
        if (previous == value) return
        if (value == null) super.remove(name) else super.put(name, value)
        val version = versions[name]
        if (version == null) versions[name] = mutableIntStateOf(0) else version.intValue++
        if (value == null) versions.remove(name)
        if (previous == null || value == null) shape.intValue++
    }
}

internal fun shareModel(previous: Any?, next: Any?): Any? = when {
    previous is JSONObject && next is JSONObject -> {
        var same = previous.length() == next.length()
        val values = next.keys().asSequence().map { name ->
            val old = previous.opt(name)
            val value = shareModel(old, next.opt(name))
            if (value !== old) same = false
            name to value
        }.toList()
        if (same) previous else JSONObject().apply { values.forEach { (name, value) -> put(name, value) } }
    }
    previous is JSONArray && next is JSONArray -> {
        var same = previous.length() == next.length()
        val values = (0 until next.length()).map { index ->
            val old = previous.opt(index)
            shareModel(old, next.opt(index)).also { if (it !== old) same = false }
        }
        if (same) previous else JSONArray(values)
    }
    previous == next -> previous
    else -> next
}
