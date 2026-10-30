package dev.dropcopy;

import android.app.Activity;
import android.content.ClipData;
import android.content.Intent;
import android.net.Uri;
import android.os.Bundle;
import java.util.ArrayList;

/** The share-sheet entry. No UI: it hands everything to SendService and closes. */
public class ShareActivity extends Activity {
    @Override
    @SuppressWarnings("deprecation")
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        Intent in = getIntent();
        ArrayList<Uri> uris = new ArrayList<>();
        String text = null;
        if (Intent.ACTION_SEND.equals(in.getAction())) {
            Uri u = in.getParcelableExtra(Intent.EXTRA_STREAM);
            if (u != null) uris.add(u);
        } else if (Intent.ACTION_SEND_MULTIPLE.equals(in.getAction())) {
            ArrayList<Uri> list = in.getParcelableArrayListExtra(Intent.EXTRA_STREAM);
            if (list != null) uris.addAll(list);
        }
        // Some apps put the files only in the ClipData.
        ClipData cd = in.getClipData();
        if (uris.isEmpty() && cd != null) {
            for (int i = 0; i < cd.getItemCount(); i++) if (cd.getItemAt(i).getUri() != null) uris.add(cd.getItemAt(i).getUri());
        }
        if (uris.isEmpty()) {
            CharSequence t = in.getCharSequenceExtra(Intent.EXTRA_TEXT);
            if (t != null) text = t.toString();
        }

        // Tapped a PC's own icon in the share sheet's top row.
        String shortcut = in.getStringExtra(Intent.EXTRA_SHORTCUT_ID);
        String pc = shortcut != null && shortcut.startsWith("pc-") ? shortcut.substring(3) : null;

        if (Store.pcs(this).isEmpty()) {
            Transfer.toast(this, "Pair your PC first");
            startActivity(new Intent(this, MainActivity.class).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
        } else if (uris.isEmpty() && text == null) {
            Transfer.toast(this, "Nothing to send");
        } else {
            SendService.start(this, uris, text, pc);
        }
        finish();
        overridePendingTransition(0, 0);
    }
}
