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
import java.net.DatagramSocket;
import java.net.Socket;
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
        return send(c, items, pcHex, progress);
    }

    static String send(Context c, List<Proto.Item> items, String pcHex, Proto.Progress progress) throws IOException {
        List<Proto.Pc> pcs = Store.pcs(c);
        if (pcHex != null) pcs.removeIf(p -> !p.hexId().equals(pcHex));
        if (pcs.isEmpty()) throw new IOException("Pair your PC first: open DropCopy");

        boolean hasOptions = false, hasFile = false;
        for (Proto.Item it : items) {
            hasOptions |= it.options() >= 0;
            hasFile |= it.options() < 0 && it.text() == null;
        }
        List<Proto.Item> toSend = items;
        if (!hasOptions && hasFile && Store.flag(c, Store.DELETE_AFTER_PASTE)) {
            toSend = new ArrayList<>(items);
            toSend.add(0, Proto.options(Proto.OPT_DELETE_AFTER_PASTE));
        }
        Proto.Result r;
        try {
            r = Proto.send(pcs, Proto.broadcastAddresses(), routes(c), toSend, progress);
        } catch (IOException e) {
            // A PC from before v0.3.3 refuses the options record: send plainly rather than fail.
            if (toSend == items || e.getMessage() == null || !e.getMessage().contains("unknown record type")) throw e;
            r = Proto.send(pcs, Proto.broadcastAddresses(), routes(c), items, progress);
        }
        Store.remember(c, r.pc);
        items = new ArrayList<>(items);
        items.removeIf(it -> it.options() >= 0);
        if (items.isEmpty()) return "Connected to " + r.pc.name + " (" + r.totalMillis + " ms)";
        String what = items.size() == 1 ? (items.get(0).text() != null ? "Text" : items.get(0).name()) : items.size() + " items";
        return what + " → " + r.pc.name + " · " + human(r.bytes) + " · " + r.totalMillis + " ms";
    }

    /**
     * Paths to race: every network the phone has, Wi-Fi first, plus the default route. When Wi-Fi
     * has no internet Android may make mobile data the default and LAN addresses unreachable
     * through it; when this phone is the hotspot only the default route reaches the PC; some
     * devices (emulators, USB/Ethernet adapters) put the LAN on an unexpected network type.
     * Racing them all needs no guess about which case this is; a dead path simply loses.
     */
    @SuppressWarnings("deprecation")
    static List<Proto.Route> routes(Context c) {
        List<Proto.Route> out = new ArrayList<>();
        ConnectivityManager cm = c.getSystemService(ConnectivityManager.class);
        if (cm != null) {
            for (final Network n : cm.getAllNetworks()) {
                NetworkCapabilities nc = cm.getNetworkCapabilities(n);
                if (nc == null) continue;
                boolean lan = nc.hasTransport(NetworkCapabilities.TRANSPORT_WIFI)
                        || nc.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET);
                out.add(lan ? 0 : out.size(), new Proto.Route() {
                    public Socket newSocket() throws IOException { return n.getSocketFactory().createSocket(); }
                    public void bind(DatagramSocket s) throws IOException { n.bindSocket(s); }
                });
            }
        }
        out.add(Proto.DEFAULT_ROUTE);
        return out;
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
