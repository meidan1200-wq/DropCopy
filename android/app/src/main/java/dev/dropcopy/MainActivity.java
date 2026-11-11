package dev.dropcopy;

import android.Manifest;
import android.app.Activity;
import android.app.Dialog;
import android.bluetooth.BluetoothAdapter;
import android.bluetooth.BluetoothManager;
import android.os.Handler;
import android.os.Looper;
import android.widget.ProgressBar;
import android.content.Context;
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
import java.io.IOException;
import java.util.ArrayList;
import java.util.List;

/** Setup only: pair with a PC, check it, choose what happens automatically. Sending never opens this. */
public class MainActivity extends Activity {
    private TextView pcName, pcDetail, statusMessage;
    private ImageView dot;
    private Button scan, check;
    private Switch shots, camera, near, updates, deleteAfter, receive;
    private View updateCard, needsCard;
    private Updater.Release pendingUpdate;
    private boolean checking, asking;
    /** Name of an unpaired PC that answered and is showing its code; null if none has. */
    private String waitingPc;
    /** The scanner opens by itself once per launch, when a PC puts its code on screen. */
    private static boolean autoScanned;
    /** The screen the user is looking at, if any: an install they started can then show at once. */
    private static java.lang.ref.WeakReference<MainActivity> current = new java.lang.ref.WeakReference<>(null);
    private final Handler ui = new Handler(Looper.getMainLooper());
    private Dialog nearDialog;
    private boolean nearAsking;

    static boolean visible() {
        return current.get() != null;
    }

    /** An install the user started was cancelled or failed: give the button back. */
    static void installEnded() {
        MainActivity a = current.get();
        if (a == null) return;
        a.runOnUiThread(() -> {
            Button b = a.findViewById(R.id.update_install);
            b.setEnabled(true);
            b.setText("Install update");
        });
    }

    @Override
    protected void onPause() {
        super.onPause();
        current = new java.lang.ref.WeakReference<>(null);
    }

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
        deleteAfter = findViewById(R.id.sw_delete);
        receive = findViewById(R.id.sw_receive);
        updateCard = findViewById(R.id.update_card);
        needsCard = findViewById(R.id.needs_card);
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
        findViewById(R.id.pc_setup).setOnClickListener(v -> showPcSetup());
        findViewById(R.id.notify_allow).setOnClickListener(v -> askNotifications());
        findViewById(R.id.oem_allow).setOnClickListener(v -> {
            android.content.Intent screen = Oem.autostart(this);
            if (screen != null) Oem.open(this, screen);
            Store.setFlag(this, OEM_SEEN, true); // there is no way to read that switch back
            refresh();
        });
        findViewById(R.id.photos_allow).setOnClickListener(v -> {
            // Asked before: Android shows its "allow all" choice only in the app's settings.
            if (Store.flag(this, ASKED_PHOTOS) && !shouldShowRequestPermissionRationale(WatchService.mediaPermission())) {
                startActivity(new Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:" + getPackageName())));
            } else {
                Store.setFlag(this, ASKED_PHOTOS, true);
                requestPermissions(new String[]{WatchService.mediaPermission()}, 5);
            }
        });
        findViewById(R.id.install_allow).setOnClickListener(v ->
                startActivity(new Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:" + getPackageName()))));
        findViewById(R.id.update_install).setOnClickListener(v -> installUpdate());
        findViewById(R.id.overlay_allow).setOnClickListener(v ->
                startActivity(new Intent(Settings.ACTION_MANAGE_OVERLAY_PERMISSION, Uri.parse("package:" + getPackageName()))));
        findViewById(R.id.phone_code).setOnClickListener(v -> showMyCode());
        findViewById(R.id.phone_scan).setOnClickListener(v -> scan());
        findViewById(R.id.phones_forget).setOnClickListener(v -> {
            for (Proto.Pc p : Store.phones(this)) Store.forgetPhone(this, p.hexId());
            WatchService.sync(this);
            refresh();
        });
        findViewById(R.id.forget).setOnClickListener(v -> {
            Store.forgetAll(this);
            for (String k : new String[]{Store.WATCH_SCREENSHOTS, Store.WATCH_CAMERA, Store.NEAR, Store.DELETE_AFTER_PASTE}) Store.setFlag(this, k, false);
            setDeleteSwitch(false);
            shots.setChecked(false);
            camera.setChecked(false);
            near.setChecked(false);
            WatchService.sync(this);
            refresh();
        });

        bind(R.id.row_shots, shots, Store.WATCH_SCREENSHOTS);
        bind(R.id.row_camera, camera, Store.WATCH_CAMERA);
        bind(R.id.row_near, near, Store.NEAR);
        // Already on: tapping the row reopens the setup and its live check; the switch turns it off.
        findViewById(R.id.row_near).setOnClickListener(v -> {
            if (near.isChecked()) showNearSetup(); else near.toggle();
        });
        receive.setChecked(Store.receive(this));
        findViewById(R.id.row_receive).setOnClickListener(v -> receive.toggle());
        receive.setOnCheckedChangeListener((b, on) -> {
            Store.setFlag(this, Store.RECEIVE, on);
            WatchService.sync(this);
            refresh();
        });
        bindDeleteAfterPaste();
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
        current = new java.lang.ref.WeakReference<>(this);
        refresh(); // e.g. back from the battery or permission settings
        if (Store.pcs(this).isEmpty()) {
            askPcToShowCode(false);
        } else {
            // Opening the app collects anything a PC sent to this phone meanwhile.
            Context app = getApplicationContext();
            new Thread(() -> { try { Receiver.collect(app); } catch (RuntimeException ignored) { } }, "collect").start();
            WatchService.sync(this);
        }
        if (!Store.phones(this).isEmpty()) WatchService.sync(this); // other phones can send while the screen is on
    }

    /**
     * Unpaired (or the PC no longer knows this phone): ask the network, so the PC puts its pairing
     * code on screen by itself. Opening the app is then all it takes to start pairing.
     */
    private void askPcToShowCode(boolean afterFailedCheck) {
        if (asking) return;
        asking = true;
        new Thread(() -> {
            String name = Proto.askToPair(Proto.broadcastAddresses(), 1500);
            runOnUiThread(() -> {
                asking = false;
                if (isFinishing()) return;
                waitingPc = name;
                if (name == null) {
                    if (!afterFailedCheck) refresh(); // keep "Can't reach the PC" on screen
                    return;
                }
                setDot(R.color.accent);
                if (afterFailedCheck) {
                    pcDetail.setText("“" + name + "” no longer knows this phone");
                    statusMessage.setVisibility(View.VISIBLE);
                    statusMessage.setText("It is showing a new pairing code above its taskbar. Tap Pair new and scan it.");
                    return;
                }
                pcName.setText("Not paired yet");
                pcDetail.setText("“" + name + "” is showing its pairing code. Point the camera at it.");
                if (!autoScanned) {
                    autoScanned = true;
                    scan();
                }
            });
        }).start();
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
            if (on && Store.NEAR.equals(key)) showNearSetup();
        });
    }

    // ---- "Delete from PC after pasting" ---------------------------------------------------

    private boolean settingDelete;

    /** Off is one tap. On asks twice, with the warning, because pictures can then be lost on the PC. */
    private void bindDeleteAfterPaste() {
        deleteAfter.setChecked(Store.flag(this, Store.DELETE_AFTER_PASTE));
        findViewById(R.id.row_delete).setOnClickListener(v -> deleteAfter.toggle());
        deleteAfter.setOnCheckedChangeListener((b, on) -> {
            if (settingDelete) return;
            if (!on) {
                Store.setFlag(this, Store.DELETE_AFTER_PASTE, false);
                return;
            }
            setDeleteSwitch(false); // stays off until both answers are yes
            new android.app.AlertDialog.Builder(this)
                    .setTitle("Delete pictures from the PC after pasting?")
                    .setMessage("Each picture you send is deleted from Pictures\\DropCopy on your PC:\n\n"
                            + "•  30 seconds after you paste it with Ctrl+V\n"
                            + "•  as soon as you copy something else on the PC first: text, a file, anything\n"
                            + "•  when the PC restarts before you paste it\n\n"
                            + "If you don't paste it in time, it is gone from the PC. The original stays on this phone.")
                    .setNegativeButton("Cancel", null)
                    .setPositiveButton("Continue", (d1, w1) -> new android.app.AlertDialog.Builder(this)
                            .setTitle("Are you sure?")
                            .setMessage("Deleted pictures don't go to the Recycle Bin and can't be recovered on the PC.")
                            .setNegativeButton("Keep them", null)
                            .setPositiveButton("Turn on", (d2, w2) -> {
                                Store.setFlag(this, Store.DELETE_AFTER_PASTE, true);
                                setDeleteSwitch(true);
                            })
                            .show())
                    .show();
        });
    }

    private void setDeleteSwitch(boolean on) {
        settingDelete = true;
        deleteAfter.setChecked(on);
        settingDelete = false;
    }

    // ---- Set up the PC from the phone ------------------------------------------------------

    private PcSetup.Server setupServer;

    /**
     * No DropCopy on the PC yet: this phone makes the key, serves the PC app with it, and once the
     * PC has downloaded it, waits for that PC to come up. See PcSetup.
     */
    private void showPcSetup() {
        String address = PcSetup.localAddress();
        if (address == null) {
            Transfer.toast(this, "Connect this phone to Wi-Fi first (or turn on its hotspot for the PC)");
            return;
        }
        Dialog d = new Dialog(this);
        d.requestWindowFeature(android.view.Window.FEATURE_NO_TITLE);
        d.setContentView(R.layout.dialog_pc_setup);
        if (d.getWindow() != null) {
            d.getWindow().setBackgroundDrawableResource(android.R.color.transparent);
            d.getWindow().setLayout(Math.min(getResources().getDisplayMetrics().widthPixels - dp(32), dp(420)), android.view.ViewGroup.LayoutParams.WRAP_CONTENT);
        }
        TextView status = d.findViewById(R.id.setup_status);
        ImageView sdot = d.findViewById(R.id.setup_dot);
        Button done = d.findViewById(R.id.setup_done);
        ((TextView) d.findViewById(R.id.setup_address)).setText("http://" + address + ":" + PcSetup.PORT);
        tint(sdot, R.color.idle);
        String link = PcSetup.newLink();
        boolean[] open = {true};
        done.setOnClickListener(v -> d.dismiss());
        d.setOnDismissListener(x -> {
            open[0] = false;
            if (setupServer != null) setupServer.stop();
            setupServer = null;
            refresh();
        });
        d.show();
        new Thread(() -> {
            try {
                byte[] exe = PcSetup.withLink(PcSetup.fetchExe(this), link);
                String page = PcSetup.page(this, Transfer.deviceName(this), exe.length);
                PcSetup.Server server = new PcSetup.Server(exe, page, who -> runOnUiThread(() -> {
                    if (!open[0]) return;
                    // Downloaded: from now on this phone knows that PC, and waits for it to start.
                    if (Store.pcs(this).isEmpty()) Store.add(this, link);
                    tint(sdot, R.color.accent);
                    status.setText("Downloaded by " + who + ". Open DropCopy-Setup.exe on the PC.");
                    waitForNewPc(d, status, sdot, done, open);
                }));
                runOnUiThread(() -> {
                    if (!open[0]) {
                        server.stop();
                        return;
                    }
                    setupServer = server;
                    tint(sdot, R.color.ok);
                    status.setText("Ready. Open the address on your PC.");
                });
            } catch (IOException e) {
                runOnUiThread(() -> {
                    tint(sdot, R.color.warn);
                    status.setText("Couldn't get the PC app (" + e.getMessage() + "). The phone needs internet once for this.");
                });
            }
        }).start();
    }

    private void waitForNewPc(Dialog d, TextView status, ImageView sdot, Button done, boolean[] open) {
        new Thread(() -> {
            for (int i = 0; i < 150 && open[0]; i++) { // ~5 minutes
                try {
                    Transfer.run(this, null, null, null, null);
                    runOnUiThread(() -> {
                        if (setupServer != null) setupServer.stop();
                        setupServer = null;
                        tint(sdot, R.color.ok);
                        status.setText("Your PC is set up and connected.");
                        done.setText("Done");
                    });
                    return;
                } catch (IOException e) {
                    try { Thread.sleep(2000); } catch (InterruptedException ignored) { return; }
                }
            }
        }).start();
    }

    // ---- "Show on PC when near" setup ------------------------------------------------------

    /** A small window over the app: what proximity needs, and whether the laptop sees the phone. */
    private void showNearSetup() {
        if (nearDialog != null && nearDialog.isShowing()) return;
        Dialog d = new Dialog(this);
        d.requestWindowFeature(android.view.Window.FEATURE_NO_TITLE);
        d.setContentView(R.layout.dialog_near);
        if (d.getWindow() != null) {
            d.getWindow().setBackgroundDrawableResource(android.R.color.transparent);
            int w = Math.min(getResources().getDisplayMetrics().widthPixels - dp(32), dp(420));
            d.getWindow().setLayout(w, android.view.ViewGroup.LayoutParams.WRAP_CONTENT);
        }
        d.findViewById(R.id.near_bt_btn).setOnClickListener(v -> startActivity(new Intent(Settings.ACTION_BLUETOOTH_SETTINGS)));
        d.findViewById(R.id.near_perm_btn).setOnClickListener(v -> {
            if (Build.VERSION.SDK_INT >= 31) requestPermissions(new String[]{Manifest.permission.BLUETOOTH_ADVERTISE}, 3);
        });
        d.findViewById(R.id.near_photo_btn).setOnClickListener(v -> requestPermissions(new String[]{WatchService.mediaPermission()}, 4));
        d.findViewById(R.id.near_done).setOnClickListener(v -> d.dismiss());
        d.setOnDismissListener(x -> {
            ui.removeCallbacks(nearPoll);
            nearDialog = null;
        });
        nearDialog = d;
        d.show();
        ui.post(nearPoll);
    }

    private int dp(int v) {
        return Math.round(v * getResources().getDisplayMetrics().density);
    }

    private final Runnable nearPoll = new Runnable() {
        @Override
        public void run() {
            if (nearDialog == null) return;
            updateNearDialog(null, false);
            ui.postDelayed(this, 1200);
        }
    };

    private boolean bluetoothOn() {
        BluetoothManager bm = getSystemService(BluetoothManager.class);
        BluetoothAdapter a = bm == null ? null : bm.getAdapter();
        try {
            return a != null && a.isEnabled();
        } catch (SecurityException e) {
            return false;
        }
    }

    private boolean advertiseAllowed() {
        return Build.VERSION.SDK_INT < 31 || checkSelfPermission(Manifest.permission.BLUETOOTH_ADVERTISE) == PackageManager.PERMISSION_GRANTED;
    }

    /** Refreshes the rows; asks the PC unless {@code answered} (then {@code st} is its answer). */
    private void updateNearDialog(Proto.NearStatus st, boolean answered) {
        Dialog d = nearDialog;
        if (d == null) return;
        boolean bt = bluetoothOn(), perm = advertiseAllowed();
        tint(d.findViewById(R.id.near_bt_dot), bt ? R.color.ok : R.color.warn);
        ((TextView) d.findViewById(R.id.near_bt_detail)).setText(bt ? "On" : "Off");
        d.findViewById(R.id.near_bt_btn).setVisibility(bt ? View.GONE : View.VISIBLE);
        tint(d.findViewById(R.id.near_perm_dot), perm ? R.color.ok : R.color.warn);
        ((TextView) d.findViewById(R.id.near_perm_detail)).setText(perm ? "Allowed" : "Lets the laptop see this phone");
        d.findViewById(R.id.near_perm_btn).setVisibility(perm ? View.GONE : View.VISIBLE);
        boolean photos = checkSelfPermission(WatchService.mediaPermission()) == PackageManager.PERMISSION_GRANTED;
        tint(d.findViewById(R.id.near_photo_dot), photos ? R.color.ok : R.color.warn);
        ((TextView) d.findViewById(R.id.near_photo_detail)).setText(photos ? "Allowed" : "Lets it send your newest photo");
        d.findViewById(R.id.near_photo_btn).setVisibility(photos ? View.GONE : View.VISIBLE);

        TextView seen = d.findViewById(R.id.near_seen_detail);
        ProgressBar meter = d.findViewById(R.id.near_meter);
        List<Proto.Pc> pcs = Store.pcs(this);
        if (pcs.isEmpty() || !bt || !perm) {
            tint(d.findViewById(R.id.near_seen_dot), R.color.idle);
            seen.setText(pcs.isEmpty() ? "Pair with your PC first." : !bt ? "Turn on Bluetooth first." : "Allow Nearby devices first.");
            meter.setVisibility(View.INVISIBLE);
            return;
        }
        if (!answered) {
            if (nearAsking) return;
            nearAsking = true;
            Proto.Pc pc = pcs.get(0);
            new Thread(() -> {
                Proto.NearStatus s = Proto.askNear(pc, Proto.broadcastAddresses(), 900);
                runOnUiThread(() -> {
                    nearAsking = false;
                    updateNearDialog(s, true);
                });
            }).start();
            return;
        }
        int dot;
        String text;
        Integer dbm = st == null ? null : st.dbm;
        if (st == null) {
            dot = R.color.warn;
            text = "Can't reach your PC. Is DropCopy running on it, on this Wi-Fi?";
        } else if (st.state == Proto.NearStatus.OFF) {
            dot = R.color.warn;
            text = "Turned off on the PC. On the PC run: dropcopy set near on";
        } else if (st.state == Proto.NearStatus.NO_BLUETOOTH) {
            dot = R.color.warn;
            text = "Your laptop's Bluetooth is off. Turn it on in Windows quick settings.";
        } else if (dbm == null) {
            dot = R.color.idle;
            text = "Not seen yet. Hold the phone right next to the laptop.";
        } else if (dbm >= st.threshold) {
            dot = R.color.ok;
            text = "Sees this phone · " + dbm + " dBm. Close enough: your newest photo pops up.";
        } else {
            dot = R.color.accent;
            text = "Sees this phone · " + dbm + " dBm. A bit closer and it pops up (at " + st.threshold + ").";
        }
        tint(d.findViewById(R.id.near_seen_dot), dot);
        seen.setText(text);
        meter.setVisibility(dbm == null ? View.INVISIBLE : View.VISIBLE);
        if (dbm != null) meter.setProgress(Math.max(0, Math.min(60, dbm + 100)));
    }

    private void tint(ImageView v, int colorRes) {
        v.setImageTintList(ColorStateList.valueOf(getColor(colorRes)));
    }

    private void handle(Intent intent) {
        Uri data = intent == null ? null : intent.getData();
        if (data != null && "dropcopy".equals(data.getScheme())) pair(data.toString());
    }

    private void setDot(int colorRes) {
        dot.setImageTintList(ColorStateList.valueOf(getColor(colorRes)));
    }

    private void refresh() {
        refreshPhones();
        List<Proto.Pc> pcs = Store.pcs(this);
        boolean paired = !pcs.isEmpty();
        check.setVisibility(paired ? View.VISIBLE : View.GONE);
        check.setText("Scan new PC"); // opens the scanner: another PC, or this one again
        scan.setText(paired ? "Check connection" : "Scan pairing code");
        findViewById(R.id.paste_toggle).setVisibility(paired ? View.GONE : View.VISIBLE);
        findViewById(R.id.pc_setup).setVisibility(paired ? View.GONE : View.VISIBLE);
        if (!checking) {
            if (paired) {
                Proto.Pc pc = pcs.get(0);
                pcName.setText(pcs.size() == 1 ? pc.name : pc.name + "  +" + (pcs.size() - 1));
                pcDetail.setText(pc.host != null ? "Last seen at " + pc.host : "Not reached yet");
            } else if (waitingPc != null) {
                pcName.setText("Not paired yet");
                pcDetail.setText("“" + waitingPc + "” is showing its pairing code. Point the camera at it.");
                statusMessage.setVisibility(View.GONE);
            } else {
                pcName.setText("Not paired yet");
                pcDetail.setText("Open DropCopy on your PC. Its pairing code appears there.");
                statusMessage.setVisibility(View.GONE);
            }
            setDot(!paired && waitingPc != null ? R.color.accent : R.color.idle);
        }
        refreshNeeds(paired);
    }

    /**
     * The settings only the user can grant, each shown while it is missing and it matters. Hidden
     * until paired, so the first screen is only about pairing.
     */
    private void refreshNeeds(boolean paired) {
        boolean phones = !Store.phones(this).isEmpty();
        paired |= phones;
        boolean background = shots.isChecked() || camera.isChecked() || near.isChecked() || receive.isChecked() || phones;
        PowerManager pm = getSystemService(PowerManager.class);
        boolean exempt = pm == null || pm.isIgnoringBatteryOptimizations(getPackageName());
        boolean notifications = Build.VERSION.SDK_INT < 33
                || checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED;
        boolean auto = Store.autoUpdate(this);
        // Partial access ("Select photos", Android 14+) or refused: new photos stay invisible.
        boolean wantsPhotos = shots.isChecked() || camera.isChecked() || near.isChecked();
        boolean photosOk = checkSelfPermission(WatchService.mediaPermission()) == PackageManager.PERMISSION_GRANTED;
        boolean oem = background && !Store.flag(this, OEM_SEEN) && Oem.autostart(this) != null;
        if (oem) {
            ((TextView) findViewById(R.id.oem_detail)).setText(Oem.brandName()
                    + " stops apps that aren't allowed to start on their own. Find DropCopy and turn it on.");
        }
        boolean[] show = {
                background && !exempt,
                oem,
                (background || auto) && !notifications,
                wantsPhotos && !photosOk,
                auto && !getPackageManager().canRequestPackageInstalls(),
                phones && !WarpOverlay.allowed(this),
        };
        int[] rows = {R.id.row_ok_battery, R.id.row_ok_oem, R.id.row_ok_notify, R.id.row_ok_photos, R.id.row_ok_install, R.id.row_ok_overlay};
        int[] dividers = {R.id.row_ok_battery_div, R.id.row_ok_oem_div, R.id.row_ok_notify_div, R.id.row_ok_photos_div, R.id.row_ok_install_div, R.id.row_ok_overlay_div};
        boolean any = false;
        for (int i = 0; i < rows.length; i++) {
            boolean on = paired && show[i];
            findViewById(rows[i]).setVisibility(on ? View.VISIBLE : View.GONE);
            findViewById(dividers[i]).setVisibility(on && any ? View.VISIBLE : View.GONE);
            any |= on;
        }
        needsCard.setVisibility(any ? View.VISIBLE : View.GONE);
    }

    private void askNotifications() {
        if (Build.VERSION.SDK_INT < 33) return;
        // Asked before and refused: Android won't ask again, so open the switch itself.
        if (Store.flag(this, ASKED_NOTIFY) && !shouldShowRequestPermissionRationale(Manifest.permission.POST_NOTIFICATIONS)) {
            startActivity(new Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS).putExtra(Settings.EXTRA_APP_PACKAGE, getPackageName()));
            return;
        }
        Store.setFlag(this, ASKED_NOTIFY, true);
        requestPermissions(new String[]{Manifest.permission.POST_NOTIFICATIONS}, 2);
    }

    private static final String ASKED_NOTIFY = "asked_notify";
    private static final String OEM_SEEN = "oem_autostart_seen";
    private static final String ASKED_PHOTOS = "asked_photos";

    private void showStatus(boolean ok, String title, String detail) {
        setDot(ok ? R.color.ok : R.color.warn);
        pcDetail.setText(title);
        statusMessage.setVisibility(detail == null ? View.GONE : View.VISIBLE);
        statusMessage.setText(detail);
    }

    private void scan() {
        try {
            GmsBarcodeScanning.getClient(this, new GmsBarcodeScannerOptions.Builder().setBarcodeFormats(Barcode.FORMAT_QR_CODE).build())
                    .startScan()
                    .addOnSuccessListener(b -> pair(b.getRawValue()))
                    .addOnFailureListener(e -> scannerUnavailable());
        } catch (RuntimeException e) { // no Play services scanner on this phone
            scannerUnavailable();
        }
    }

    private void scannerUnavailable() {
        findViewById(R.id.paste_toggle).setVisibility(View.VISIBLE);
        findViewById(R.id.paste_row).setVisibility(View.VISIBLE);
        Transfer.toast(this, "Scanner unavailable here - paste the link the PC shows");
    }

    private void pair(String link) {
        try {
            if (Proto.Pc.parse(link).phone) {
                pairPhone(Proto.Pc.parse(link));
                return;
            }
            Proto.Pc pc = Store.add(this, link);
            waitingPc = null;
            findViewById(R.id.paste_row).setVisibility(View.GONE);
            refresh();
            requestPermissionsIfNeeded();
            WatchService.sync(this); // receiving from PCs starts with the pairing, not the next app start
            Transfer.toast(this, "Paired with " + pc.name);
            check();
        } catch (IllegalArgumentException e) {
            Transfer.toast(this, "That isn't a DropCopy pairing code");
        }
    }

    // ---- other phones ----------------------------------------------------------------------

    private void refreshPhones() {
        List<Proto.Pc> phones = Store.phones(this);
        TextView title = findViewById(R.id.phones_title), detail = findViewById(R.id.phones_detail);
        if (phones.isEmpty()) {
            title.setText("Send to another phone");
            detail.setText("Pictures warp from one phone to the other. Both need DropCopy and the same Wi-Fi.");
        } else {
            List<String> names = new ArrayList<>();
            for (Proto.Pc p : phones) names.add(p.name);
            title.setText(String.join(", ", names));
            detail.setText("Share → its icon at the top of the share sheet. It receives while its screen is on.");
        }
        findViewById(R.id.phones_forget).setVisibility(phones.isEmpty() ? View.GONE : View.VISIBLE);
    }

    /** Scanned another phone's code: remember it and say hello, so it knows this phone back. */
    private void pairPhone(Proto.Pc phone) {
        Store.rememberPhone(this, phone);
        WatchService.sync(this);
        refresh();
        String hex = phone.hexId();
        Context app = getApplicationContext();
        new Thread(() -> {
            String msg;
            try {
                Transfer.sendToPhone(app, new ArrayList<>(), hex, null);
                msg = "Paired with " + phone.name + ". Share a picture to it from the share sheet.";
            } catch (IOException e) {
                msg = "Saved " + phone.name + ", but couldn't reach it. Keep its code on screen, on the same Wi-Fi, and scan again.";
            }
            Transfer.toast(app, msg);
            runOnUiThread(this::refresh);
        }, "pair-phone").start();
    }

    /** This phone's code, for another phone to scan. It listens while the code is on screen. */
    private void showMyCode() {
        if (PcSetup.localAddress() == null) {
            Transfer.toast(this, "Connect to Wi-Fi first: both phones need the same one");
            return;
        }
        PhoneServer.hold(this, "code");
        android.graphics.Bitmap qr = qr(Store.me(this).toLink(), dp(240));
        Dialog d = new Dialog(this);
        d.requestWindowFeature(android.view.Window.FEATURE_NO_TITLE);
        d.setContentView(R.layout.dialog_my_code);
        if (d.getWindow() != null) {
            d.getWindow().setBackgroundDrawableResource(android.R.color.transparent);
            d.getWindow().setLayout(Math.min(getResources().getDisplayMetrics().widthPixels - dp(32), dp(420)), android.view.ViewGroup.LayoutParams.WRAP_CONTENT);
        }
        ((ImageView) d.findViewById(R.id.code_image)).setImageBitmap(qr);
        ((TextView) d.findViewById(R.id.code_title)).setText(Transfer.deviceName(this));
        TextView status = d.findViewById(R.id.code_status);
        ImageView cdot = d.findViewById(R.id.code_dot);
        tint(cdot, R.color.accent);
        d.findViewById(R.id.code_done).setOnClickListener(v -> d.dismiss());
        PhoneServer.onPaired = name -> runOnUiThread(() -> {
            tint(cdot, R.color.ok);
            status.setText("Paired with " + name);
            refresh();
            WatchService.sync(this);
            new Handler(Looper.getMainLooper()).postDelayed(() -> { if (d.isShowing()) d.dismiss(); }, 1500);
        });
        d.setOnDismissListener(x -> {
            PhoneServer.onPaired = null;
            PhoneServer.release("code");
            WatchService.sync(this);
        });
        d.show();
    }

    /** A QR code: black on white with its quiet zone, {@code size} pixels square. */
    static android.graphics.Bitmap qr(String text, int size) {
        try {
            java.util.Map<com.google.zxing.EncodeHintType, Object> hints = new java.util.EnumMap<>(com.google.zxing.EncodeHintType.class);
            hints.put(com.google.zxing.EncodeHintType.MARGIN, 2);
            hints.put(com.google.zxing.EncodeHintType.ERROR_CORRECTION, com.google.zxing.qrcode.decoder.ErrorCorrectionLevel.M);
            com.google.zxing.common.BitMatrix m = new com.google.zxing.qrcode.QRCodeWriter()
                    .encode(text, com.google.zxing.BarcodeFormat.QR_CODE, size, size, hints);
            int[] px = new int[m.getWidth() * m.getHeight()];
            for (int y = 0; y < m.getHeight(); y++) {
                for (int x = 0; x < m.getWidth(); x++) px[y * m.getWidth() + x] = m.get(x, y) ? 0xFF16181B : 0xFFFFFFFF;
            }
            return android.graphics.Bitmap.createBitmap(px, m.getWidth(), m.getHeight(), android.graphics.Bitmap.Config.ARGB_8888);
        } catch (com.google.zxing.WriterException e) {
            return null;
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
                            "Is DropCopy running on it, on the same Wi-Fi? On Windows, allow it through the firewall.");
                    // Maybe the PC is there but was reset or re-keyed: if so it shows its code now.
                    askPcToShowCode(true);
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
            Store.setFlag(this, ASKED_NOTIFY, true);
        }
        if ((shots.isChecked() || camera.isChecked() || near.isChecked()) && checkSelfPermission(WatchService.mediaPermission()) != PackageManager.PERMISSION_GRANTED) {
            want.add(WatchService.mediaPermission()); // "near" sends the newest photo
        }
        if (near.isChecked() && Build.VERSION.SDK_INT >= 31 && checkSelfPermission(Manifest.permission.BLUETOOTH_ADVERTISE) != PackageManager.PERMISSION_GRANTED) {
            want.add(Manifest.permission.BLUETOOTH_ADVERTISE);
        }
        if (!want.isEmpty()) requestPermissions(want.toArray(new String[0]), 1);
    }

    @Override
    public void onRequestPermissionsResult(int code, String[] perms, int[] results) {
        WatchService.sync(this);
        if (nearDialog != null) updateNearDialog(null, false);
        if ((shots.isChecked() || camera.isChecked()) && checkSelfPermission(WatchService.mediaPermission()) != PackageManager.PERMISSION_GRANTED) {
            Transfer.toast(this, "Sending new screenshots needs access to all photos");
        }
        refresh();
    }
}
