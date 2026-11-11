package dev.dropcopy;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.Service;
import android.content.ClipData;
import android.content.Context;
import android.content.Intent;
import android.net.Uri;
import android.os.IBinder;
import android.os.SystemClock;
import java.util.ArrayList;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

/** Runs transfers in the background so the share sheet can close immediately. */
public class SendService extends Service {
    private static final String CHANNEL = "send";
    private final ExecutorService exec = Executors.newSingleThreadExecutor();

    /** Starts a transfer. The URI read grants travel with the intent. */
    static void start(Context c, ArrayList<Uri> uris, String text, String pcHex) {
        Intent s = new Intent(c, SendService.class)
                .putParcelableArrayListExtra("uris", uris)
                .putExtra("text", text)
                .putExtra("pc", pcHex);
        if (!uris.isEmpty()) {
            ClipData cd = ClipData.newRawUri("", uris.get(0));
            for (int i = 1; i < uris.size(); i++) cd.addItem(new ClipData.Item(uris.get(i)));
            s.setClipData(cd);
            s.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
        }
        c.startForegroundService(s);
    }

    /**
     * Collects what a PC sent to this phone ({@code pcHex}), or from every PC that says something
     * is waiting (null). Runs as a short foreground job, like a send.
     */
    static void fetch(Context c, String pcHex) {
        Intent s = new Intent(c, SendService.class).setAction(ACTION_FETCH).putExtra("pc", pcHex);
        try {
            c.startForegroundService(s);
        } catch (RuntimeException e) {
            // Android refused a background start: it is collected the next time the app runs.
        }
    }

    static final String ACTION_FETCH = "dev.dropcopy.FETCH";

    static Notification.Builder notification(Context c, String channel, String title) {
        NotificationManager nm = c.getSystemService(NotificationManager.class);
        nm.createNotificationChannel(new NotificationChannel(channel,
                CHANNEL.equals(channel) ? "Transfers" : "Auto-send", NotificationManager.IMPORTANCE_LOW));
        return new Notification.Builder(c, channel).setSmallIcon(R.drawable.ic_notify).setContentTitle(title).setOngoing(true);
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        boolean fetching = intent != null && ACTION_FETCH.equals(intent.getAction());
        startForeground(1, notification(this, CHANNEL, fetching ? "Receiving from PC…" : "Sending to PC…").build());
        if (intent == null) {
            stopSelf(startId);
            return START_NOT_STICKY;
        }
        if (fetching) {
            String hex = intent.getStringExtra("pc");
            exec.execute(() -> {
                try {
                    if (hex == null) {
                        Receiver.collect(this);
                    } else {
                        for (Proto.Pc pc : Store.pcs(this)) if (pc.hexId().equals(hex)) Receiver.fetch(this, pc);
                    }
                } catch (Exception ignored) {
                    // Still on the PC: it is collected next time.
                } finally {
                    stopSelf(startId);
                }
            });
            return START_NOT_STICKY;
        }
        @SuppressWarnings("deprecation")
        ArrayList<Uri> uris = intent.getParcelableArrayListExtra("uris");
        String text = intent.getStringExtra("text");
        String pc = intent.getStringExtra("pc");
        exec.execute(() -> {
            NotificationManager nm = getSystemService(NotificationManager.class);
            long[] last = {0};
            Proto.Progress progress = (sent, total) -> {
                long now = SystemClock.elapsedRealtime();
                if (total <= 0 || now - last[0] < 300) return;
                last[0] = now;
                nm.notify(1, notification(this, CHANNEL, "Sending to PC…")
                        .setProgress(1000, (int) (sent * 1000 / total), false).build());
            };
            try {
                Transfer.toast(this, "✓ " + Transfer.run(this, uris, text, pc, progress));
            } catch (Proto.Unreachable e) {
                try {
                    Outbox.add(this, uris, text, pc);
                    Transfer.toast(this, "PC not reachable - it will send by itself when you're back on your Wi-Fi");
                } catch (Exception q) {
                    Transfer.toast(this, "Not sent: " + e.getMessage());
                }
            } catch (Exception e) {
                Transfer.toast(this, "Not sent: " + e.getMessage()
                        + "\nIs dropcopy running on the PC, on the same Wi-Fi?");
            } finally {
                stopSelf(startId);
            }
        });
        return START_NOT_STICKY;
    }

    @Override
    public void onDestroy() {
        exec.shutdown();
        super.onDestroy();
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }
}
