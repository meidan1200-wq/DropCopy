package dev.dropcopy;

import android.app.Activity;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.net.Uri;
import android.os.Bundle;
import java.util.ArrayList;

/**
 * Sends whatever is on the phone's clipboard. Android only lets the focused app read the
 * clipboard, so this invisible activity takes focus for an instant, reads it, and closes.
 */
public class ClipActivity extends Activity {
    private boolean done;

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        getWindow().getDecorView().postDelayed(() -> { if (!done) finish(); }, 2000);
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        if (!hasFocus || done) return;
        done = true;
        ClipboardManager cm = getSystemService(ClipboardManager.class);
        ClipData clip = cm == null ? null : cm.getPrimaryClip();
        ArrayList<Uri> uris = new ArrayList<>();
        String text = null;
        if (clip != null) {
            for (int i = 0; i < clip.getItemCount(); i++) {
                ClipData.Item it = clip.getItemAt(i);
                if (it.getUri() != null) uris.add(it.getUri());
                else if (text == null && it.coerceToText(this) != null) text = it.coerceToText(this).toString();
            }
        }
        if (uris.isEmpty() && (text == null || text.isEmpty())) {
            Transfer.toast(this, "The clipboard is empty");
        } else if (Store.pcs(this).isEmpty()) {
            Transfer.toast(this, "Pair your PC first: open DropCopy");
        } else {
            SendService.start(this, uris, uris.isEmpty() ? text : null, getIntent().getStringExtra("pc"));
        }
        finish();
        overridePendingTransition(0, 0);
    }
}
