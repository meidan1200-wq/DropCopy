package dev.dropcopy;

import android.Manifest;
import android.app.Service;
import android.bluetooth.BluetoothAdapter;
import android.bluetooth.BluetoothManager;
import android.bluetooth.le.AdvertiseData;
import android.bluetooth.le.AdvertisingSet;
import android.bluetooth.le.AdvertisingSetCallback;
import android.bluetooth.le.AdvertisingSetParameters;
import android.bluetooth.le.BluetoothLeAdvertiser;
import android.content.BroadcastReceiver;
import android.content.IntentFilter;
import android.os.PowerManager;
import android.content.ContentUris;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.database.ContentObserver;
import android.database.Cursor;
import android.net.Uri;
import android.os.Build;
import android.os.Handler;
import android.os.HandlerThread;
import android.os.IBinder;
import android.provider.MediaStore;
import java.util.Collections;
import java.util.HashSet;
import java.util.Locale;
import java.util.Set;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

/**
 * Auto-send: take a screenshot (or a photo) on the phone and it is on the PC's clipboard a
 * moment later, with no tap at all. Android forbids reading the clipboard in the background,
 * but a new image appearing in MediaStore can be observed, so this is the zero-tap path.
 */
public class WatchService extends Service {
    private static final String CHANNEL = "watch";
    private final ExecutorService exec = Executors.newSingleThreadExecutor();
    private final Set<Long> seen = new HashSet<>();
    private HandlerThread thread;
    private Handler handler;
    private ContentObserver observer;
    private long sinceSeconds;

    static String mediaPermission() {
        return Build.VERSION.SDK_INT >= 33 ? Manifest.permission.READ_MEDIA_IMAGES : Manifest.permission.READ_EXTERNAL_STORAGE;
    }

    static boolean wantsMedia(Context c) {
        return (Store.flag(c, Store.WATCH_SCREENSHOTS) || Store.flag(c, Store.WATCH_CAMERA))
                && c.checkSelfPermission(mediaPermission()) == PackageManager.PERMISSION_GRANTED;
    }

    static boolean wantsNear(Context c) {
        return Store.flag(c, Store.NEAR)
                && (Build.VERSION.SDK_INT < 31
                    || c.checkSelfPermission(Manifest.permission.BLUETOOTH_ADVERTISE) == PackageManager.PERMISSION_GRANTED);
    }

    /** Starts or stops the background service to match the settings. */
    static void sync(Context c) {
        boolean want = (wantsMedia(c) || wantsNear(c)) && !Store.pcs(c).isEmpty();
        Intent i = new Intent(c, WatchService.class);
        if (want) {
            try { c.startForegroundService(i); } catch (RuntimeException ignored) { }
        } else {
            c.stopService(i);
        }
    }

    private final Runnable scan = this::scan;

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        boolean media = wantsMedia(this), near = wantsNear(this);
        String what = media && near ? "Sending new screenshots · visible to your PC when near"
                : media ? "Sending new screenshots to your PC" : "Your PC will see the phone when it's near";
        startForeground(2, SendService.notification(this, CHANNEL, what).setContentText("Change in DropCopy").build());
        if (near) startNearWatch(); else stopNearWatch();
        if (media && observer == null) {
            sinceSeconds = System.currentTimeMillis() / 1000 - 2;
            thread = new HandlerThread("watch");
            thread.start();
            handler = new Handler(thread.getLooper());
            observer = new ContentObserver(handler) {
                @Override
                public void onChange(boolean selfChange, Uri uri) {
                    // A new image fires several changes (insert, then publish): act once they settle.
                    handler.removeCallbacks(scan);
                    handler.postDelayed(scan, 250);
                }
            };
            getContentResolver().registerContentObserver(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, true, observer);
        }
        return START_STICKY;
    }

    private void scan() {
        boolean shots = Store.flag(this, Store.WATCH_SCREENSHOTS), camera = Store.flag(this, Store.WATCH_CAMERA);
        Uri base = MediaStore.Images.Media.EXTERNAL_CONTENT_URI;
        String[] cols = {MediaStore.Images.Media._ID, MediaStore.Images.Media.RELATIVE_PATH, MediaStore.Images.Media.IS_PENDING};
        try (Cursor c = getContentResolver().query(base, cols, MediaStore.Images.Media.DATE_ADDED + " >= ?",
                new String[]{String.valueOf(sinceSeconds)}, MediaStore.Images.Media.DATE_ADDED + " DESC")) {
            if (c == null) return;
            for (int n = 0; c.moveToNext() && n < 20; n++) {
                long id = c.getLong(0);
                String path = c.isNull(1) ? "" : c.getString(1).toLowerCase(Locale.ROOT);
                if (seen.contains(id) || c.getInt(2) == 1) continue; // already sent, or still being written
                boolean isShot = path.contains("screenshot");
                boolean isCamera = path.startsWith("dcim/camera");
                if (!((isShot && shots) || (isCamera && camera))) continue;
                seen.add(id);
                Uri uri = ContentUris.withAppendedId(base, id);
                exec.execute(() -> {
                    try {
                        Transfer.toast(this, "✓ " + Transfer.run(this, Collections.singletonList(uri), null, null, null));
                    } catch (Proto.Unreachable e) {
                        // Off Wi-Fi: queue silently; it goes out when the network returns.
                        try { Outbox.add(this, Collections.singletonList(uri), null, null); } catch (Exception ignored) { }
                    } catch (Exception e) {
                        Transfer.toast(this, "Auto-send failed: " + e.getMessage());
                    }
                });
            }
        } catch (RuntimeException ignored) {
            // Permission revoked while running: settings sync will stop the service.
        }
    }

    // ---- "bring the phone close" -------------------------------------------------------------

    private AdvertisingSet adSet;
    private boolean advertising;
    private BroadcastReceiver nearEvents;
    private Handler main;
    private final Runnable rotate = new Runnable() {
        @Override
        public void run() {
            if (adSet != null) {
                try { adSet.setAdvertisingData(nearData()); } catch (SecurityException ignored) { }
            }
            main.postDelayed(this, msToNextMinute());
        }
    };

    private final AdvertisingSetCallback adCallback = new AdvertisingSetCallback() {
        @Override
        public void onAdvertisingSetStarted(AdvertisingSet set, int txPower, int status) {
            adSet = status == ADVERTISE_SUCCESS ? set : null;
        }

        @Override
        public void onAdvertisingSetStopped(AdvertisingSet set) {
            adSet = null;
        }
    };

    private static long msToNextMinute() {
        return 60_000 - System.currentTimeMillis() % 60_000 + 50;
    }

    private AdvertiseData nearData() {
        java.util.List<Proto.Pc> pcs = Store.pcs(this);
        byte[] key = pcs.isEmpty() ? new byte[32] : pcs.get(0).key;
        return new AdvertiseData.Builder()
                .addManufacturerData(Proto.NEAR_COMPANY_ID, Proto.nearPayload(key, System.currentTimeMillis() / 60_000))
                .build();
    }

    /** Advertises only while the screen is on: that is when the phone is in a hand. */
    private void startNearWatch() {
        if (nearEvents != null) return;
        main = new Handler(android.os.Looper.getMainLooper());
        nearEvents = new BroadcastReceiver() {
            @Override
            public void onReceive(Context c, Intent i) {
                updateAdvertising();
            }
        };
        IntentFilter f = new IntentFilter();
        f.addAction(Intent.ACTION_SCREEN_ON);
        f.addAction(Intent.ACTION_SCREEN_OFF);
        f.addAction(BluetoothAdapter.ACTION_STATE_CHANGED);
        registerReceiver(nearEvents, f);
        updateAdvertising();
    }

    private void stopNearWatch() {
        if (nearEvents == null) return;
        unregisterReceiver(nearEvents);
        nearEvents = null;
        setAdvertising(false);
    }

    private void updateAdvertising() {
        PowerManager pm = getSystemService(PowerManager.class);
        setAdvertising(pm != null && pm.isInteractive());
    }

    private void setAdvertising(boolean on) {
        BluetoothManager bm = getSystemService(BluetoothManager.class);
        BluetoothAdapter adapter = bm == null ? null : bm.getAdapter();
        BluetoothLeAdvertiser adv = adapter != null && adapter.isEnabled() ? adapter.getBluetoothLeAdvertiser() : null;
        try {
            if (on && adv != null && !advertising) {
                AdvertisingSetParameters p = new AdvertisingSetParameters.Builder()
                        .setLegacyMode(true)
                        .setConnectable(false)
                        .setScannable(false)
                        .setInterval(AdvertisingSetParameters.INTERVAL_MEDIUM) // ~4 per second
                        .setTxPowerLevel(AdvertisingSetParameters.TX_POWER_MEDIUM)
                        .build();
                adv.startAdvertisingSet(p, nearData(), null, null, null, adCallback);
                advertising = true;
                main.removeCallbacks(rotate);
                main.postDelayed(rotate, msToNextMinute());
            } else if ((!on || adv == null) && advertising) {
                if (adv != null) adv.stopAdvertisingSet(adCallback);
                advertising = false;
                adSet = null;
                main.removeCallbacks(rotate);
            }
        } catch (SecurityException | IllegalStateException e) {
            advertising = false; // permission revoked or Bluetooth went away
        }
    }

    @Override
    public void onDestroy() {
        stopNearWatch();
        if (observer != null) getContentResolver().unregisterContentObserver(observer);
        if (thread != null) thread.quitSafely();
        exec.shutdown();
        super.onDestroy();
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }
}
