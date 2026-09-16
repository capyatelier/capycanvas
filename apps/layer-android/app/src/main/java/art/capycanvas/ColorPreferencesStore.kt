package art.capycanvas

import android.content.Context
import android.util.AtomicFile
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.io.File

/** Application preferences are serialized on IO, independently of a document. */
internal object ColorPreferencesStore {
    private val lock=Any()
    @Volatile internal var directoryForTest:File?=null
    suspend fun presets(context:Context,color:JSONObject,request:JSONObject):JSONObject=withContext(Dispatchers.IO){
        synchronized(lock){
            val file=File(directoryForTest?:context.filesDir,"color-export-presets.json")
            val atomic=AtomicFile(file)
            val bytes=try{atomic.openRead().use {input->check(file.length()<=64L*1024*1024){"Export presets exceed 64 MiB"};input.readBytes()}}
                catch(e:java.io.FileNotFoundException){ByteArray(0)}
            val result=Native.exportPresets(bytes,request.toString(),color.toString())
            val next=result[1] as ByteArray?
            if(next!=null){val output=atomic.startWrite();try{output.write(next);atomic.finishWrite(output)}catch(e:Exception){atomic.failWrite(output);throw e}}
            JSONObject(result[0] as String)
        }
    }
}
