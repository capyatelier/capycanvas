package art.capycanvas

import android.content.Context
import android.util.AtomicFile
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.io.File
import java.security.MessageDigest

/** Imported ICCs are exact app-owned copies. Deletion never touches source files. */
internal object ProfileStore {
    private const val MAX_PROFILE=16L*1024*1024
    private const val MAX_TOTAL=64L*1024*1024
    private val lock=Any()
    private val idPattern=Regex("[a-f0-9]{64}")
    private fun root(context:Context)=File(ColorPreferencesStore.directoryForTest?:context.filesDir,"color-profiles")
    private fun files(context:Context)=root(context).listFiles()?.filter{it.isFile&&it.extension=="icc"&&idPattern.matches(it.nameWithoutExtension)}?:emptyList()
    private fun digest(bytes:ByteArray)=MessageDigest.getInstance("SHA-256").digest(bytes).joinToString(""){"%02x".format(it)}
    private fun read(file:File):ByteArray {
        check(file.length()<=MAX_PROFILE){"ICC profile exceeds 16 MiB"}
        val bytes=file.readBytes();check(digest(bytes)==file.nameWithoutExtension){"Profile changed on disk; remove or reimport it"};return bytes
    }
    suspend fun list(context:Context):List<JSONObject> = withContext(Dispatchers.IO){synchronized(lock){
        var total=0L
        files(context).sortedBy{it.name}.take(128).map{file->
            total+=file.length()
            val entry=try{check(total<=MAX_TOTAL){"Library exceeds 64 MiB; remove unused profiles"};JSONObject(Native.inspectProfileSummary(read(file)))}
                catch(e:Exception){obj("name" to "Unavailable profile ${file.nameWithoutExtension.take(12)}","issue" to (e.message?:"Invalid ICC profile"))}
            entry.put("id",file.nameWithoutExtension).put("bytes",file.length())
        }.sortedBy{it.getString("name")}
    }}
    suspend fun import(context:Context,bytes:ByteArray):JSONObject=withContext(Dispatchers.IO){synchronized(lock){
        check(bytes.size<=MAX_PROFILE){"ICC profile exceeds 16 MiB"}
        Native.inspectProfileSummary(bytes)
        val id=digest(bytes);val other=files(context).filter{it.nameWithoutExtension!=id}
        check(other.size<128&&other.sumOf{it.length()}+bytes.size<=MAX_TOTAL){"The profile library limit is 128 profiles and 64 MiB"}
        val file=AtomicFile(File(root(context),"$id.icc"));val output=file.startWrite()
        try{output.write(bytes);file.finishWrite(output)}catch(e:Exception){file.failWrite(output);throw e}
        JSONObject(Native.inspectProfile(bytes))
    }}
    suspend fun get(context:Context,id:String):JSONObject=withContext(Dispatchers.IO){synchronized(lock){
        check(idPattern.matches(id)){"Select an imported profile"};JSONObject(Native.inspectProfile(read(File(root(context),"$id.icc"))))
    }}
    suspend fun remove(context:Context,id:String)=withContext(Dispatchers.IO){synchronized(lock){
        check(idPattern.matches(id)){"Select an imported profile"};val file=File(root(context),"$id.icc")
        check(file.exists()&&file.delete()){ "Could not remove the imported profile" }
    }}
}
