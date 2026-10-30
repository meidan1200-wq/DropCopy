package dev.dropcopy;

import android.Manifest;
import android.app.Activity;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.graphics.Typeface;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.text.InputType;
import android.view.View;
import android.widget.Button;
import android.widget.CompoundButton;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.Switch;
import android.widget.TextView;
import com.google.mlkit.vision.barcode.common.Barcode;
import com.google.mlkit.vision.codescanner.GmsBarcodeScanner;
import com.google.mlkit.vision.codescanner.GmsBarcodeScannerOptions;
import com.google.mlkit.vision.codescanner.GmsBarcodeScanning;
import java.util.ArrayList;
import java.util.List;

/** Setup only: pair with a PC, test the connection, toggle auto-send. Sending never opens this. */
public class MainActivity extends Activity {
    private TextView status;
    private Switch shots, camera;

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        int pad = (int) (20 * getResources().getDisplayMetrics().density);
        LinearLayout col = new LinearLayout(this);
        col.setOrientation(LinearLayout.VERTICAL);
        col.setPadding(pad, pad, pad, pad);

        TextView title = new TextView(this);
        title.setText("DropCopy");
        title.setTextSize(28);
        title.setTypeface(Typeface.DEFAULT_BOLD);
        col.addView(title);

        status = new TextView(this);
        status.setTextSize(16);
        status.setPadding(0, pad / 2, 0, pad);
        col.addView(status);

        col.addView(button("Scan pairing code", v -> scan()));
        EditText link = new EditText(this);
        link.setHint("…or paste the pairing link");
        link.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_URI);
        link.setSingleLine(true);
        col.addView(link);
        col.addView(button("Pair with pasted link", v -> pair(link.getText().toString())));
        col.addView(button("Check connection", v -> check()));

        shots = new Switch(this);
        shots.setText("Auto-send new screenshots");
        shots.setPadding(0, pad, 0, pad / 2);
        col.addView(shots);
        camera = new Switch(this);
        camera.setText("Auto-send new camera photos");
        camera.setPadding(0, pad / 2, 0, pad);
        col.addView(camera);

        TextView help = new TextView(this);
        help.setText("How to send\n"
                + "• Share → DropCopy, or tap your PC's own icon in the share sheet's top row.\n"
                + "• Clipboard: add the \"Send clipboard\" tile to Quick Settings and tap it after copying.\n"
                + "• Zero taps: turn on auto-send above, then just take a screenshot.\n\n"
                + "On the PC, run dropcopy. Files land in Pictures\\DropCopy and on the clipboard: press Ctrl+V.\n"
                + "Phone and PC must be on the same Wi-Fi (or the PC on the phone's hotspot). Nothing leaves your network.");
        help.setPadding(0, 0, 0, pad);
        col.addView(help);

        col.addView(button("Forget paired PCs", v -> {
            Store.forgetAll(this);
            Store.setFlag(this, Store.WATCH_SCREENSHOTS, false);
            Store.setFlag(this, Store.WATCH_CAMERA, false);
            WatchService.sync(this);
            refresh();
        }));

        ScrollView scroll = new ScrollView(this);
        scroll.addView(col);
        scroll.setFitsSystemWindows(true);
        setContentView(scroll);

        CompoundButton.OnCheckedChangeListener onToggle = (b, on) -> {
            Store.setFlag(this, b == shots ? Store.WATCH_SCREENSHOTS : Store.WATCH_CAMERA, on);
            if (on) requestPermissionsIfNeeded();
            WatchService.sync(this);
        };
        shots.setChecked(Store.flag(this, Store.WATCH_SCREENSHOTS));
        camera.setChecked(Store.flag(this, Store.WATCH_CAMERA));
        shots.setOnCheckedChangeListener(onToggle);
        camera.setOnCheckedChangeListener(onToggle);

        handle(getIntent());
        refresh();
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        handle(intent);
    }

    private void handle(Intent intent) {
        Uri data = intent == null ? null : intent.getData();
        if (data != null && "dropcopy".equals(data.getScheme())) pair(data.toString());
    }

    private Button button(String text, View.OnClickListener l) {
        Button b = new Button(this);
        b.setText(text);
        b.setAllCaps(false);
        b.setOnClickListener(l);
        return b;
    }

    private void refresh() {
        List<Proto.Pc> pcs = Store.pcs(this);
        if (pcs.isEmpty()) {
            status.setText("Not paired yet.\nRun dropcopy on your PC and scan the code it shows.");
            return;
        }
        StringBuilder sb = new StringBuilder("Paired with:");
        for (Proto.Pc pc : pcs) {
            sb.append("\n• ").append(pc.name);
            if (pc.host != null) sb.append("  (last seen at ").append(pc.host).append(')');
        }
        status.setText(sb);
    }

    private void scan() {
        GmsBarcodeScannerOptions opts = new GmsBarcodeScannerOptions.Builder()
                .setBarcodeFormats(Barcode.FORMAT_QR_CODE).build();
        GmsBarcodeScanner scanner = GmsBarcodeScanning.getClient(this, opts);
        scanner.startScan()
                .addOnSuccessListener(b -> pair(b.getRawValue()))
                .addOnFailureListener(e -> Transfer.toast(this, "Scanner unavailable - paste the link instead"));
    }

    private void pair(String link) {
        try {
            Proto.Pc pc = Store.add(this, link);
            Transfer.toast(this, "Paired with " + pc.name + ". Checking…");
            refresh();
            requestPermissionsIfNeeded();
            check();
        } catch (IllegalArgumentException e) {
            Transfer.toast(this, "That is not a DropCopy pairing code: " + e.getMessage());
        }
    }

    /** An empty transfer: proves discovery, the key and the PC's firewall without touching its clipboard. */
    private void check() {
        status.setText("Looking for your PC…");
        new Thread(() -> {
            String msg;
            try {
                msg = "✓ " + Transfer.run(this, null, null, null, null);
            } catch (Exception e) {
                msg = "✗ " + e.getMessage() + "\nIs dropcopy running on the PC, on the same Wi-Fi? "
                        + "On Windows, allow it through the firewall on private networks.";
            }
            String m = msg;
            runOnUiThread(() -> {
                refresh();
                status.append("\n\n" + m);
            });
        }).start();
    }

    private void requestPermissionsIfNeeded() {
        List<String> want = new ArrayList<>();
        if (Build.VERSION.SDK_INT >= 33 && checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
            want.add(Manifest.permission.POST_NOTIFICATIONS);
        }
        if ((shots.isChecked() || camera.isChecked()) && checkSelfPermission(WatchService.mediaPermission()) != PackageManager.PERMISSION_GRANTED) {
            want.add(WatchService.mediaPermission());
        }
        if (!want.isEmpty()) requestPermissions(want.toArray(new String[0]), 1);
    }

    @Override
    public void onRequestPermissionsResult(int code, String[] perms, int[] results) {
        WatchService.sync(this);
        if ((shots.isChecked() || camera.isChecked())
                && checkSelfPermission(WatchService.mediaPermission()) != PackageManager.PERMISSION_GRANTED) {
            Transfer.toast(this, "Auto-send needs access to all photos");
        }
    }
}
