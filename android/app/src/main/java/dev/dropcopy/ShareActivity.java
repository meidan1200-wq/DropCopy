package dev.dropcopy;

import android.app.Activity;
import android.content.ClipData;
import android.content.Intent;
import android.net.Uri;
import android.os.Bundle;
import java.util.ArrayList;

/**
 * The share-sheet entry. It hands everything to SendService and closes; a picture sent to
 * another phone first warps away into the top of the screen.
 */
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

        // The icon in the share sheet's top row picks the PC by itself (null). An icon for one PC
        // ("pc-<id>") came from an older version; the system may still offer it for a while.
        String shortcut = in.getStringExtra(Intent.EXTRA_SHORTCUT_ID);
        String pc = shortcut != null && shortcut.startsWith("pc-") ? shortcut.substring(3) : null;
        if (pc != null && Store.pcs(this).size() > 1) pc = null;

        // A paired phone's icon, or no PC but a phone: to that phone, and a picture warps away.
        String phone = shortcut != null && shortcut.startsWith(Store.PHONE_SHORTCUT) ? shortcut.substring(Store.PHONE_SHORTCUT.length()) : null;
        if (phone == null && pc == null && Store.pcs(this).isEmpty() && !Store.phones(this).isEmpty()) {
            phone = Store.phones(this).get(0).hexId();
        }
        if (phone != null && (!uris.isEmpty() || text != null)) {
            SendService.start(this, uris, text, SendService.PHONE + phone);
            Uri picture = null;
            for (Uri u : uris) {
                String mime = getContentResolver().getType(u);
                if (mime != null && mime.startsWith("image/")) {
                    picture = u;
                    break;
                }
            }
            WarpView warp = picture == null ? null : WarpView.create(this, picture, true);
            if (warp != null) {
                setContentView(warp);
                warp.play(() -> {
                    finish();
                    overridePendingTransition(0, 0);
                });
                return;
            }
            finish();
            overridePendingTransition(0, 0);
            return;
        }

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
