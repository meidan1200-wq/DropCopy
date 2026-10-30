package dev.dropcopy;

import android.content.ContentResolver;
import android.content.Context;
import android.database.Cursor;
import android.net.ConnectivityManager;
import android.net.Network;
import android.net.NetworkCapabilities;
import android.net.Uri;
import android.os.Handler;
import android.os.Looper;
import android.provider.OpenableColumns;
import android.webkit.MimeTypeMap;
import android.widget.Toast;
import java.io.FileNotFoundException;
import java.io.IOException;
import java.io.InputStream;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;

/** Turns content URIs and text into protocol items and sends them to a paired PC. */
final class Transfer {
    private Transfer() {}

    static Proto.Item item(Context c, Uri uri) {
        ContentResolver cr = c.getContentResolver();
        String mime = cr.getType(uri);
        if (mime == null) mime = "application/octet-stream";
        String name = null;
        long size = -1;
        try (Cursor cur = cr.query(uri, new String[]{OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE}, null, null, null)) {
            if (cur != null && cur.moveToFirst()) {
                int n = cur.getColumnIndex(OpenableColumns.DISPLAY_NAME), s = cur.getColumnIndex(OpenableColumns.SIZE);
                if (n >= 0 && !cur.isNull(n)) name = cur.getString(n);
                if (s >= 0 && !cur.isNull(s)) size = cur.getLong(s);
            }
        } catch (RuntimeException ignored) {
            // Some providers don't answer queries; the stream still works.
        }
        if (name == null || name.isEmpty()) {
            String ext = MimeTypeMap.getSingleton().getExtensionFromMimeType(mime);
            name = "shared_" + System.currentTimeMillis() + (ext != null ? "." + ext : "");
        }
        final String fName = name, fMime = mime;
        final long fSize = size;
        return new Proto.Item() {
            public String name() { return fName; }
            public String mime() { return fMime; }
            public long size() { return fSize; }
            public InputStream open() throws IOException {
                InputStream is = cr.openInputStream(uri);
                if (is == null) throw new FileNotFoundException(fName);
                return is;
            }
        };
    }

    /** Sends and returns a one-line summary. {@code pcHex} null means "whichever PC answers first". */
    static String run(Context c, List<Uri> uris, String text, String pcHex, Proto.Progress progress) throws IOException {
        List<Proto.Item> items = new ArrayList<>();
        if (uris != null) for (Uri u : uris) items.add(item(c, u));
        if (text != null && !text.isEmpty()) items.add(Proto.text(text));

        List<Proto.Pc> pcs = Store.pcs(c);
        if (pcHex != null) pcs.removeIf(p -> !p.hexId().equals(pcHex));
        if (pcs.isEmpty()) throw new IOException("Pair your PC first: open DropCopy");

        bindToLan(c);
        Proto.Result r = Proto.send(pcs, Proto.broadcastAddresses(), items, progress);
        Store.remember(c, r.pc);
        if (items.isEmpty()) return "Connected to " + r.pc.name + " (" + r.totalMillis + " ms)";
        String what = items.size() == 1 ? (items.get(0).text() != null ? "Text" : items.get(0).name()) : items.size() + " items";
        return what + " → " + r.pc.name + " · " + human(r.bytes) + " · " + r.totalMillis + " ms";
    }

    /**
     * When Wi-Fi has no internet, Android may make mobile data the default network and LAN
     * addresses become unreachable. Pin this app to Wi-Fi (or Ethernet) whenever one is up.
     */
    @SuppressWarnings("deprecation")
    static void bindToLan(Context c) {
        ConnectivityManager cm = c.getSystemService(ConnectivityManager.class);
        if (cm == null) return;
        for (Network n : cm.getAllNetworks()) {
            NetworkCapabilities nc = cm.getNetworkCapabilities(n);
            if (nc != null && (nc.hasTransport(NetworkCapabilities.TRANSPORT_WIFI)
                    || nc.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET))) {
                cm.bindProcessToNetwork(n);
                return;
            }
        }
        cm.bindProcessToNetwork(null); // e.g. this phone is the hotspot: use the normal routes
    }

    static String human(long n) {
        if (n < 1024) return n + " B";
        if (n < 1024 * 1024) return String.format(Locale.US, "%.0f KB", n / 1024.0);
        return String.format(Locale.US, "%.1f MB", n / 1048576.0);
    }

    static void toast(Context c, String msg) {
        new Handler(Looper.getMainLooper()).post(() -> Toast.makeText(c.getApplicationContext(), msg, Toast.LENGTH_SHORT).show());
    }
}
