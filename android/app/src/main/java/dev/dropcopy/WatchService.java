package dev.dropcopy;

import android.Manifest;
import android.app.Service;
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

    /** Starts or stops the watcher to match the settings. */
    static void sync(Context c) {
        boolean want = (Store.flag(c, Store.WATCH_SCREENSHOTS) || Store.flag(c, Store.WATCH_CAMERA))
                && c.checkSelfPermission(mediaPermission()) == PackageManager.PERMISSION_GRANTED
                && !Store.pcs(c).isEmpty();
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
        startForeground(2, SendService.notification(this, CHANNEL, "Auto-sending new screenshots to your PC")
                .setContentText("Turn off in DropCopy").build());
        if (observer == null) {
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
                    } catch (Exception e) {
                        Transfer.toast(this, "Auto-send failed: " + e.getMessage());
                    }
                });
            }
        } catch (RuntimeException ignored) {
            // Permission revoked while running: settings sync will stop the service.
        }
    }

    @Override
    public void onDestroy() {
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
