package dev.dropcopy;

import android.Manifest;
import android.app.Activity;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.content.res.ColorStateList;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.PowerManager;
import android.provider.Settings;
import android.view.View;
import android.widget.Button;
import android.widget.EditText;
import android.widget.ImageView;
import android.widget.Switch;
import android.widget.TextView;
import com.google.mlkit.vision.barcode.common.Barcode;
import com.google.mlkit.vision.codescanner.GmsBarcodeScannerOptions;
import com.google.mlkit.vision.codescanner.GmsBarcodeScanning;
import java.util.ArrayList;
import java.util.List;

/** Setup only: pair with a PC, check it, choose what happens automatically. Sending never opens this. */
public class MainActivity extends Activity {
    private TextView pcName, pcDetail, statusMessage;
    private ImageView dot;
    private Button scan, check;
    private Switch shots, camera, near, updates;
    private View batteryRow, updateCard;
    private Updater.Release pendingUpdate;
    private boolean checking;

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        setContentView(R.layout.activity_main);
        pcName = findViewById(R.id.pc_name);
        pcDetail = findViewById(R.id.pc_detail);
        statusMessage = findViewById(R.id.status_message);
        dot = findViewById(R.id.status_dot);
        scan = findViewById(R.id.scan);
        check = findViewById(R.id.check);
        shots = findViewById(R.id.sw_shots);
        camera = findViewById(R.id.sw_camera);
        near = findViewById(R.id.sw_near);
        updates = findViewById(R.id.sw_updates);
        batteryRow = findViewById(R.id.row_battery);
        updateCard = findViewById(R.id.update_card);
        ((TextView) findViewById(R.id.version)).setText("v" + BuildConfig.VERSION_NAME);

        // Unpaired: [Scan pairing code]. Paired: [Check connection] [Pair new], so the everyday
        // action is the prominent one. The two buttons swap roles rather than styles.
        scan.setOnClickListener(v -> {
            if (Store.pcs(this).isEmpty()) scan(); else check();
        });
        check.setOnClickListener(v -> scan());
        View pasteRow = findViewById(R.id.paste_row);
        findViewById(R.id.paste_toggle).setOnClickListener(v -> pasteRow.setVisibility(pasteRow.getVisibility() == View.VISIBLE ? View.GONE : View.VISIBLE));
        EditText link = findViewById(R.id.link);
        findViewById(R.id.pair).setOnClickListener(v -> pair(link.getText().toString()));
        findViewById(R.id.battery_allow).setOnClickListener(v -> askBatteryExemption());
        findViewById(R.id.update_install).setOnClickListener(v -> installUpdate());
        findViewById(R.id.forget).setOnClickListener(v -> {
            Store.forgetAll(this);
            for (String k : new String[]{Store.WATCH_SCREENSHOTS, Store.WATCH_CAMERA, Store.NEAR}) Store.setFlag(this, k, false);
            shots.setChecked(false);
            camera.setChecked(false);
            near.setChecked(false);
            WatchService.sync(this);
            refresh();
        });

        bind(R.id.row_shots, shots, Store.WATCH_SCREENSHOTS);
        bind(R.id.row_camera, camera, Store.WATCH_CAMERA);
        bind(R.id.row_near, near, Store.NEAR);
        updates.setChecked(Store.autoUpdate(this));
        findViewById(R.id.row_updates).setOnClickListener(v -> updates.toggle());
        updates.setOnCheckedChangeListener((b, on) -> {
            Store.setFlag(this, Store.AUTO_UPDATE, on);
            Updater.schedule(this);
        });

        handle(getIntent());
        refresh();
        Updater.schedule(this);
        checkForUpdate();
    }

    @Override
    protected void onResume() {
        super.onResume();
        refresh(); // e.g. back from the battery or permission settings
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        handle(intent);
    }

    /** A switch row: the whole row toggles, and the setting starts or stops the background service. */
    private void bind(int rowId, Switch sw, String key) {
        sw.setChecked(Store.flag(this, key));
        findViewById(rowId).setOnClickListener(v -> sw.toggle());
        sw.setOnCheckedChangeListener((b, on) -> {
            Store.setFlag(this, key, on);
            if (on) requestPermissionsIfNeeded();
            WatchService.sync(this);
            refresh();
        });
    }

    private void handle(Intent intent) {
        Uri data = intent == null ? null : intent.getData();
        if (data != null && "dropcopy".equals(data.getScheme())) pair(data.toString());
    }

    private void setDot(int colorRes) {
        dot.setImageTintList(ColorStateList.valueOf(getColor(colorRes)));
    }

    private void refresh() {
        List<Proto.Pc> pcs = Store.pcs(this);
        boolean paired = !pcs.isEmpty();
        check.setVisibility(paired ? View.VISIBLE : View.GONE);
        check.setText("Pair new");
        scan.setText(paired ? "Check connection" : "Scan pairing code");
        findViewById(R.id.paste_toggle).setVisibility(paired ? View.GONE : View.VISIBLE);
        if (!checking) {
            if (paired) {
                Proto.Pc pc = pcs.get(0);
                pcName.setText(pcs.size() == 1 ? pc.name : pc.name + "  +" + (pcs.size() - 1));
                pcDetail.setText(pc.host != null ? "Last seen at " + pc.host : "Not reached yet");
            } else {
                pcName.setText("Not paired yet");
                pcDetail.setText("Run dropcopy on your PC, then scan the code it shows");
                statusMessage.setVisibility(View.GONE);
            }
            setDot(R.color.idle);
        }
        boolean background = shots.isChecked() || camera.isChecked() || near.isChecked();
        PowerManager pm = getSystemService(PowerManager.class);
        boolean exempt = pm == null || pm.isIgnoringBatteryOptimizations(getPackageName());
        batteryRow.setVisibility(background && !exempt ? View.VISIBLE : View.GONE);
    }

    private void showStatus(boolean ok, String title, String detail) {
        setDot(ok ? R.color.ok : R.color.warn);
        pcDetail.setText(title);
        statusMessage.setVisibility(detail == null ? View.GONE : View.VISIBLE);
        statusMessage.setText(detail);
    }

    private void scan() {
        GmsBarcodeScanning.getClient(this, new GmsBarcodeScannerOptions.Builder().setBarcodeFormats(Barcode.FORMAT_QR_CODE).build())
                .startScan()
                .addOnSuccessListener(b -> pair(b.getRawValue()))
                .addOnFailureListener(e -> {
                    findViewById(R.id.paste_toggle).setVisibility(View.VISIBLE);
                    findViewById(R.id.paste_row).setVisibility(View.VISIBLE);
                    Transfer.toast(this, "Scanner unavailable here - paste the link the PC shows");
                });
    }

    private void pair(String link) {
        try {
            Proto.Pc pc = Store.add(this, link);
            findViewById(R.id.paste_row).setVisibility(View.GONE);
            refresh();
            requestPermissionsIfNeeded();
            Transfer.toast(this, "Paired with " + pc.name);
            check();
        } catch (IllegalArgumentException e) {
            Transfer.toast(this, "That isn't a DropCopy pairing code");
        }
    }

    /** An empty transfer: proves discovery, the key and the PC's firewall without touching its clipboard. */
    private void check() {
        if (Store.pcs(this).isEmpty() || checking) return;
        checking = true;
        setDot(R.color.idle);
        pcDetail.setText("Looking for your PC…");
        statusMessage.setVisibility(View.GONE);
        new Thread(() -> {
            long ms = -1;
            String err = null;
            try {
                long t0 = System.currentTimeMillis();
                Transfer.run(this, null, null, null, null);
                ms = System.currentTimeMillis() - t0;
            } catch (Exception e) {
                err = e.getMessage();
            }
            long fMs = ms;
            String fErr = err;
            runOnUiThread(() -> {
                checking = false;
                refresh();
                if (fErr == null) {
                    showStatus(true, "Connected · " + fMs + " ms", null);
                } else {
                    showStatus(false, "Can't reach the PC",
                            "Is dropcopy running on it, on the same Wi-Fi? On Windows, allow it through the firewall.");
                }
            });
        }).start();
    }

    private void checkForUpdate() {
        new Thread(() -> {
            try {
                Updater.Release r = Updater.check();
                if (r == null) return;
                runOnUiThread(() -> {
                    pendingUpdate = r;
                    ((TextView) findViewById(R.id.update_title)).setText("Update available: v" + r.version);
                    TextView notes = findViewById(R.id.update_notes);
                    String n = r.notes == null ? "" : r.notes.replaceAll("[*`#]", "").trim();
                    notes.setText(n.isEmpty() ? "A newer version of DropCopy is ready." : n);
                    updateCard.setVisibility(View.VISIBLE);
                });
            } catch (Exception ignored) {
                // Offline: nothing to show.
            }
        }).start();
    }

    private void installUpdate() {
        Updater.Release r = pendingUpdate;
        if (r == null) return;
        if (!getPackageManager().canRequestPackageInstalls()) {
            Transfer.toast(this, "Allow DropCopy to install updates, then tap Install again");
            startActivity(new Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:" + getPackageName())));
            return;
        }
        Button b = findViewById(R.id.update_install);
        b.setEnabled(false);
        b.setText("Downloading…");
        new Thread(() -> {
            try {
                Updater.install(this, Updater.download(this, r));
                runOnUiThread(() -> b.setText("Installing…"));
            } catch (Exception e) {
                runOnUiThread(() -> {
                    b.setEnabled(true);
                    b.setText("Try again");
                    Transfer.toast(this, "Update failed: " + e.getMessage());
                });
            }
        }).start();
    }

    @android.annotation.SuppressLint("BatteryLife") // the user turned on background work; this keeps it alive
    private void askBatteryExemption() {
        startActivity(new Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, Uri.parse("package:" + getPackageName())));
    }

    private void requestPermissionsIfNeeded() {
        List<String> want = new ArrayList<>();
        if (Build.VERSION.SDK_INT >= 33 && checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
            want.add(Manifest.permission.POST_NOTIFICATIONS);
        }
        if ((shots.isChecked() || camera.isChecked()) && checkSelfPermission(WatchService.mediaPermission()) != PackageManager.PERMISSION_GRANTED) {
            want.add(WatchService.mediaPermission());
        }
        if (near.isChecked() && Build.VERSION.SDK_INT >= 31 && checkSelfPermission(Manifest.permission.BLUETOOTH_ADVERTISE) != PackageManager.PERMISSION_GRANTED) {
            want.add(Manifest.permission.BLUETOOTH_ADVERTISE);
        }
        if (!want.isEmpty()) requestPermissions(want.toArray(new String[0]), 1);
    }

    @Override
    public void onRequestPermissionsResult(int code, String[] perms, int[] results) {
        WatchService.sync(this);
        if ((shots.isChecked() || camera.isChecked()) && checkSelfPermission(WatchService.mediaPermission()) != PackageManager.PERMISSION_GRANTED) {
            Transfer.toast(this, "Sending new screenshots needs access to all photos");
        }
        refresh();
    }
}
