package dev.dropcopy;

import android.app.PendingIntent;
import android.content.Context;
import android.content.Intent;
import android.net.ConnectivityManager;
import android.net.NetworkCapabilities;
import android.net.NetworkRequest;
import android.net.Uri;
import android.webkit.MimeTypeMap;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;

/**
 * Sends that could not reach the PC wait here and go out on their own once the phone is back on
 * a network: no error to dismiss, nothing to redo. Each batch is a folder of copied files (the
 * share sheet's read grant would expire) plus an optional ".text" and ".pc" (target PC id).
 */
final class Outbox {
    private Outbox() {}

    private static File root(Context c) {
        File d = new File(c.getFilesDir(), "outbox");
        d.mkdirs();
        return d;
    }

    static synchronized void add(Context c, List<Uri> uris, String text, String pcHex) throws IOException {
        File batch = new File(root(c), String.valueOf(System.nanoTime()));
        if (!batch.mkdirs()) throw new IOException("cannot queue");
        try {
            if (uris != null) {
                for (Uri u : uris) {
                    Proto.Item it = Transfer.item(c, u);
                    File dest = new File(batch, safeName(it.name()));
                    try (InputStream in = it.open(); OutputStream out = new FileOutputStream(dest)) {
                        byte[] buf = new byte[256 * 1024];
                        for (int n; (n = in.read(buf)) > 0; ) out.write(buf, 0, n);
                    }
                }
            }
            if (text != null && !text.isEmpty()) write(new File(batch, ".text"), text);
            if (pcHex != null) write(new File(batch, ".pc"), pcHex);
        } catch (IOException e) {
            delete(batch);
            throw e;
        }
        RetryJob.schedule(c);
        watchNetwork(c, true);
    }

    static synchronized boolean isEmpty(Context c) {
        String[] b = root(c).list();
        return b == null || b.length == 0;
    }

    /** Sends every queued batch, oldest first. Returns how many went out. */
    static synchronized int flush(Context c) throws IOException {
        File[] batches = root(c).listFiles();
        if (batches == null) return 0;
        Arrays.sort(batches);
        int sent = 0;
        for (File batch : batches) {
            List<Proto.Item> items = new ArrayList<>();
            File[] files = batch.listFiles();
            if (files == null) continue;
            Arrays.sort(files);
            for (final File f : files) {
                if (f.getName().startsWith(".")) continue;
                final String ext = MimeTypeMap.getFileExtensionFromUrl(Uri.fromFile(f).toString());
                final String mime = MimeTypeMap.getSingleton().getMimeTypeFromExtension(ext);
                items.add(new Proto.Item() {
                    public String name() { return f.getName(); }
                    public String mime() { return mime != null ? mime : "application/octet-stream"; }
                    public long size() { return f.length(); }
                    public InputStream open() throws IOException { return new FileInputStream(f); }
                });
            }
            File text = new File(batch, ".text"), pc = new File(batch, ".pc");
            if (text.exists()) items.add(Proto.text(read(text)));
            Transfer.send(c, items, pc.exists() ? read(pc) : null, null);
            delete(batch);
            sent++;
        }
        watchNetwork(c, false);
        return sent;
    }

    /**
     * While something is queued, Android wakes us whenever any network comes up, even if the app
     * isn't running; JobScheduler's own retries back off, this catches "just rejoined my Wi-Fi".
     */
    static void watchNetwork(Context c, boolean on) {
        ConnectivityManager cm = c.getSystemService(ConnectivityManager.class);
        if (cm == null) return;
        PendingIntent pi = PendingIntent.getBroadcast(c, 0, new Intent(c, NetReceiver.class),
                PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_MUTABLE);
        try {
            cm.unregisterNetworkCallback(pi);
        } catch (RuntimeException ignored) {
            // Not registered.
        }
        if (on) {
            NetworkRequest req = new NetworkRequest.Builder()
                    .addCapability(NetworkCapabilities.NET_CAPABILITY_NOT_RESTRICTED).build();
            cm.registerNetworkCallback(req, pi);
        }
    }

    private static String safeName(String n) {
        String s = n.replace('/', '_').replace('\\', '_');
        while (s.startsWith(".")) s = s.substring(1);
        return s.isEmpty() ? "file" : s;
    }

    private static void write(File f, String s) throws IOException {
        Files.write(f.toPath(), s.getBytes(StandardCharsets.UTF_8));
    }

    private static String read(File f) throws IOException {
        return new String(Files.readAllBytes(f.toPath()), StandardCharsets.UTF_8);
    }

    private static void delete(File f) {
        File[] kids = f.listFiles();
        if (kids != null) for (File k : kids) delete(k);
        f.delete();
    }
}
