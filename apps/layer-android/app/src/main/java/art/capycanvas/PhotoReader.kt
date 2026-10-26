package art.capycanvas

import android.app.Service
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.os.Binder
import android.os.IBinder
import android.os.Looper
import android.os.Parcel
import android.os.ParcelFileDescriptor
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit

internal object PhotoReader {
    lateinit var context: Context
    @JvmStatic fun decode(input: Int, output: Int, errors: Int, sourceBytes: Long, codecBytes: Long, dimension: Int): Boolean {
        check(Looper.getMainLooper().thread != Thread.currentThread()) { "Photo decoding cannot wait on the main thread" }
        val connected = LinkedBlockingQueue<IBinder>()
        val connection = object : ServiceConnection {
            override fun onServiceConnected(name: ComponentName, service: IBinder) { connected.put(service) }
            override fun onServiceDisconnected(name: ComponentName) {}
        }
        check(context.bindService(Intent(context, PhotoReaderService::class.java), connection, Context.BIND_AUTO_CREATE)) { "Photo decoder is unavailable" }
        val files = intArrayOf(input, output, errors).map(ParcelFileDescriptor::fromFd)
        val request = Parcel.obtain()
        val reply = Parcel.obtain()
        try {
            val service = checkNotNull(connected.poll(30, TimeUnit.SECONDS)) { "Photo decoder is unavailable" }
            files.forEach { request.writeFileDescriptor(it.fileDescriptor) }
            request.writeLong(sourceBytes); request.writeLong(codecBytes); request.writeInt(dimension)
            service.transact(IBinder.FIRST_CALL_TRANSACTION, request, reply, 0)
            return reply.readInt() != 0
        } finally {
            request.recycle(); reply.recycle(); files.forEach(ParcelFileDescriptor::close)
            context.unbindService(connection)
        }
    }
}

class PhotoReaderService : Service() {
    override fun onBind(intent: Intent): IBinder = object : Binder() {
        override fun onTransact(code: Int, data: Parcel, reply: Parcel?, flags: Int): Boolean {
            val files = List(3) { checkNotNull(data.readFileDescriptor()).detachFd() }
            val decoded = Native.servePhoto(files[0], files[1], files[2], data.readLong(), data.readLong(), data.readInt())
            reply?.writeInt(if (decoded) 1 else 0)
            return true
        }
    }
}
