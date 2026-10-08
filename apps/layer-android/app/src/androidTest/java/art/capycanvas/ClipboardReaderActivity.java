package art.capycanvas;

import android.app.Activity;
import android.content.ClipboardManager;
import android.net.Uri;
import android.os.Bundle;
import android.os.Process;
import android.os.ResultReceiver;
import java.io.ByteArrayOutputStream;
import java.io.InputStream;

public class ClipboardReaderActivity extends Activity {
    private boolean reading;
    @Override public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        if (!hasFocus || reading) return;
        reading = true;
        ResultReceiver receiver = getIntent().getParcelableExtra("result");
        Bundle result = new Bundle();
        result.putInt("uid", Process.myUid());
        try {
            Uri uri = getSystemService(ClipboardManager.class).getPrimaryClip().getItemAt(0).getUri();
            new Thread(() -> {
                try (InputStream input = getContentResolver().openInputStream(uri);
                     ByteArrayOutputStream output = new ByteArrayOutputStream()) {
                    byte[] buffer = new byte[65536];
                    int count;
                    while ((count = input.read(buffer)) != -1) output.write(buffer, 0, count);
                    result.putByteArray("png", output.toByteArray());
                } catch (Exception error) { result.putString("error", error.toString()); }
                runOnUiThread(() -> { receiver.send(0, result); finish(); });
            }).start();
        } catch (Exception error) {
            result.putString("error", error.toString()); receiver.send(0, result); finish();
        }
    }
}
