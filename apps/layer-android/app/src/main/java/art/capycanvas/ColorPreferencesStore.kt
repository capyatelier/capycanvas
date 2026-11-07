package art.capycanvas

import android.content.Context
import android.util.AtomicFile
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject

/** Application preferences are serialized on IO, independently of a document. */
internal object ColorPreferencesStore {
    private val lock=Any()
    private fun read(atomic:AtomicFile):ByteArray=try {
        atomic.openRead().use { input ->
            if(atomic.baseFile.length()>64L*1024*1024)throw ColorFeatureFailure("PresetFileLimit")
            input.readBytes()
        }
    }catch(e:java.io.FileNotFoundException){ByteArray(0)}
    private fun write(atomic:AtomicFile,bytes:ByteArray) {
        val output=atomic.startWrite()
        try{output.write(bytes);atomic.finishWrite(output)}catch(e:Exception){atomic.failWrite(output);throw e}
    }
    suspend fun read(context:Context):ByteArray=withContext(Dispatchers.IO) {
        synchronized(lock){read(AtomicFile(AppStorage.of(context).exportPresets))}
    }
    suspend fun replace(context:Context,candidate:JSONObject)=withContext(Dispatchers.IO) {
        synchronized(lock) {
            val atomic=AtomicFile(AppStorage.of(context).exportPresets)
            fun bytes(key:String)=candidate.getJSONArray(key).let { array -> ByteArray(array.length()){array.getInt(it).toByte()} }
            write(atomic,Native.replaceExportPresets(read(atomic),bytes("expected"),bytes("next")))
        }
    }
    suspend fun presets(context:Context,color:JSONObject,request:JSONObject):JSONObject=withContext(Dispatchers.IO){
        synchronized(lock){
            val file=AppStorage.of(context).exportPresets
            val atomic=AtomicFile(file)
            val bytes=read(atomic)
            val result=Native.exportPresets(bytes,request.toString(),color.toString())
            val next=result[1] as ByteArray?
            if(next!=null)write(atomic,next)
            JSONObject(result[0] as String)
        }
    }
}
