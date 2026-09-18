package art.capycanvas

import android.content.Context
import android.util.AtomicFile
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject
import java.io.File

/** Storage and locking only. Shared Rust owns ICC identity, quotas and listing policy. */
internal object ProfileStore {
    private val lock=Any()
    private val readLimit by lazy { JSONObject(Native.profileLibrary(obj("type" to "limits").toString(),byteArrayOf())).getInt("read_bytes") }
    private fun root(context:Context)=File(ColorPreferencesStore.directoryForTest?:context.filesDir,"color-profiles")
    private fun call(action:JSONObject,bytes:ByteArray=byteArrayOf())=Native.profileLibrary(action.toString(),bytes)
    private fun inventory(context:Context):JSONArray {
        val entries=JSONArray()
        root(context).listFiles()?.filter{it.isFile&&it.extension=="icc"}?.forEach{entries.put(obj("id" to it.nameWithoutExtension,"bytes" to it.length()))}
        return JSONArray(call(obj("type" to "inventory","entries" to entries)))
    }
    private fun key(id:String)=JSONObject(call(obj("type" to "remove","id" to id))).getString("id")
    private fun visibility(context:Context,id:String?=null,visible:Boolean?=null):JSONArray {
        val file=AtomicFile(File(root(context),"menus.json"))
        val hidden=if(file.baseFile.exists())JSONArray(file.openRead().use{it.readBytes().decodeToString()}) else JSONArray()
        val next=JSONArray(call(obj("type" to "visibility","hidden" to hidden,"id" to id,"visible" to visible)))
        if(id!=null){val output=file.startWrite();try{output.write(next.toString().toByteArray());file.finishWrite(output)}catch(e:Exception){file.failWrite(output);throw e}}
        return next
    }
    suspend fun show(context:Context,id:String,visible:Boolean)=withContext(Dispatchers.IO){synchronized(lock){visibility(context,id,visible);Unit}}
    private fun read(file:File)=file.inputStream().use{input->
        val output=java.io.ByteArrayOutputStream();val block=ByteArray(64*1024);var remaining=readLimit+1
        while(remaining>0){val count=input.read(block,0,minOf(block.size,remaining));if(count<0)break;output.write(block,0,count);remaining-=count}
        output.toByteArray()
    }
    private fun profile(entry:JSONObject)=obj("name" to entry.getString("name"),"channels" to entry.getString("channels"),"profile" to entry.getJSONObject("profile"))
    suspend fun list(context:Context):List<JSONObject> = withContext(Dispatchers.IO){synchronized(lock){
        val hidden=visibility(context).values().toSet()
        inventory(context).objects().map{entry->
            var failure:String?=null
            val bytes=try{if(entry.has("issue"))byteArrayOf() else read(File(root(context),"${entry.getString("id")}.icc"))}catch(e:Exception){failure=e.message?:"Profile is unavailable";byteArrayOf()}
            JSONObject(call(obj("type" to "inspect","entry" to entry,"error" to failure),bytes)).put("visible",entry.getString("id") !in hidden)
        }.sortedWith(compareBy({it.getString("name")},{it.getString("id")}))
    }}
    suspend fun import(context:Context,bytes:ByteArray):JSONObject=withContext(Dispatchers.IO){synchronized(lock){
        val entry=JSONObject(call(obj("type" to "import","entries" to inventory(context)),bytes))
        val file=AtomicFile(File(root(context),"${entry.getString("id")}.icc"));val output=file.startWrite()
        try{output.write(bytes);file.finishWrite(output)}catch(e:Exception){file.failWrite(output);throw e}
        profile(entry)
    }}
    suspend fun get(context:Context,id:String):JSONObject=withContext(Dispatchers.IO){synchronized(lock){
        val name=key(id)
        profile(JSONObject(call(obj("type" to "get","id" to name),read(File(root(context),"$name.icc")))))
    }}
    suspend fun remove(context:Context,id:String)=withContext(Dispatchers.IO){synchronized(lock){
        val file=File(root(context),"${key(id)}.icc")
        check(file.exists()&&file.delete()){ "Could not remove the imported profile" }
        visibility(context,id,true)
    }}
}
