package dev.dropcopy;

import android.content.Context;
import java.io.BufferedReader;
import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.Inet4Address;
import java.net.InetAddress;
import java.net.InterfaceAddress;
import java.net.NetworkInterface;
import java.net.ServerSocket;
import java.net.Socket;
import java.net.URL;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.StandardCharsets;
import java.util.Base64;
import java.util.Collections;
import org.json.JSONArray;
import org.json.JSONObject;

/**
 * Setting up the PC from the phone. The phone makes the pairing key itself, fetches the PC app
 * (DropCopy-Setup.exe) from the releases once, and serves it on the local network with its pairing
 * link appended to the file. The PC's setup reads the link from its own file and takes the key, so
 * the phone, which holds it already, connects without any code to scan.
 *
 * Tail of the served file, as the PC reads it (pc/src/handoff.rs):
 * link (utf-8) | u32 little-endian length | "DCSETUP1".
 */
final class PcSetup {
    static final int PORT = 48652;
    static final String FILE = "DropCopy-Setup.exe";
    static final byte[] MAGIC = "DCSETUP1".getBytes(StandardCharsets.US_ASCII);

    interface Listener {
        /** The PC at {@code who} downloaded the setup. */
        void downloaded(String who);
    }

    private PcSetup() {}

    /** A new pairing link with a fresh id and key, for a PC that doesn't exist yet. */
    static String newLink() {
        byte[] id = Proto.random(8), key = Proto.random(32);
        StringBuilder hex = new StringBuilder();
        for (byte b : id) hex.append(String.format("%02x", b & 0xff));
        return "dropcopy://pair?v=1&id=" + hex + "&k=" + Base64.getUrlEncoder().withoutPadding().encodeToString(key)
                + "&n=Your%20PC&p=" + Proto.TCP_PORT;
    }

    /** The program with the link appended. */
    static byte[] withLink(byte[] exe, String link) {
        byte[] l = link.getBytes(StandardCharsets.UTF_8);
        ByteBuffer b = ByteBuffer.allocate(exe.length + l.length + 12).order(ByteOrder.LITTLE_ENDIAN);
        b.put(exe).put(l).putInt(l.length).put(MAGIC);
        return b.array();
    }

    /** The newest DropCopy-Setup.exe, downloaded once per version into the app's files. */
    static byte[] fetchExe(Context c) throws IOException {
        HttpURLConnection api = (HttpURLConnection) new URL(Updater.LATEST).openConnection();
        api.setConnectTimeout(8000);
        api.setReadTimeout(20000);
        api.setRequestProperty("Accept", "application/vnd.github+json");
        String url = null, tag = "latest";
        try (InputStream in = api.getInputStream()) {
            JSONObject o = new JSONObject(new String(readAll(in), StandardCharsets.UTF_8));
            tag = o.optString("tag_name", tag);
            JSONArray assets = o.getJSONArray("assets");
            for (int i = 0; i < assets.length(); i++) {
                JSONObject a = assets.getJSONObject(i);
                if (FILE.equals(a.optString("name"))) url = a.getString("browser_download_url");
            }
        } catch (org.json.JSONException e) {
            throw new IOException("unreadable release info", e);
        } finally {
            api.disconnect();
        }
        if (url == null) throw new IOException("the PC app isn't in the latest release");
        File cached = new File(c.getFilesDir(), "pc-setup-" + tag.replaceAll("[^0-9A-Za-z.]", "") + ".exe");
        if (cached.length() > 100_000) return java.nio.file.Files.readAllBytes(cached.toPath());
        HttpURLConnection dl = (HttpURLConnection) new URL(url).openConnection();
        dl.setInstanceFollowRedirects(true);
        dl.setConnectTimeout(8000);
        dl.setReadTimeout(30000);
        byte[] exe;
        try (InputStream in = dl.getInputStream()) {
            exe = readAll(in);
        } finally {
            dl.disconnect();
        }
        if (exe.length < 100_000 || exe[0] != 'M' || exe[1] != 'Z') throw new IOException("the download wasn't a Windows program");
        try (OutputStream o = new FileOutputStream(cached)) {
            o.write(exe);
        }
        return exe;
    }

    /** This phone's address on the Wi-Fi (or its own hotspot), for the PC to type in. */
    static String localAddress() {
        String fallback = null;
        try {
            for (NetworkInterface nif : Collections.list(NetworkInterface.getNetworkInterfaces())) {
                if (!nif.isUp() || nif.isLoopback()) continue;
                for (InterfaceAddress a : nif.getInterfaceAddresses()) {
                    InetAddress ip = a.getAddress();
                    if (!(ip instanceof Inet4Address) || !ip.isSiteLocalAddress()) continue;
                    String name = nif.getName();
                    if (name.startsWith("wlan") || name.startsWith("ap") || name.startsWith("swlan")) return ip.getHostAddress();
                    if (fallback == null) fallback = ip.getHostAddress();
                }
            }
        } catch (IOException ignored) { }
        return fallback;
    }

    /** Serves a page and the program until {@link #stop}. */
    static final class Server {
        private final ServerSocket sock;
        private final byte[] file;
        private final byte[] page;
        private final Listener listener;
        private volatile boolean running = true;

        Server(byte[] file, Listener listener) throws IOException {
            this(file, PAGE, listener);
        }

        /** {@code page}: the HTML shown at "/" (see {@link #page}). */
        Server(byte[] file, String page, Listener listener) throws IOException {
            this.file = file;
            this.page = page.getBytes(StandardCharsets.UTF_8);
            this.listener = listener;
            sock = new ServerSocket(PORT);
            new Thread(this::loop, "pc-setup").start();
        }

        void stop() {
            running = false;
            try { sock.close(); } catch (IOException ignored) { }
        }

        private void loop() {
            while (running) {
                try (Socket s = sock.accept()) {
                    s.setSoTimeout(10_000);
                    BufferedReader r = new BufferedReader(new InputStreamReader(s.getInputStream(), StandardCharsets.US_ASCII));
                    String req = r.readLine();
                    for (String h; (h = r.readLine()) != null && !h.isEmpty(); ) { /* headers: not needed */ }
                    String path = req == null ? "/" : req.split(" ").length > 1 ? req.split(" ")[1] : "/";
                    OutputStream out = s.getOutputStream();
                    if (path.startsWith("/" + FILE)) {
                        out.write(("HTTP/1.1 200 OK\r\nContent-Type: application/vnd.microsoft.portable-executable\r\n"
                                + "Content-Disposition: attachment; filename=\"" + FILE + "\"\r\nContent-Length: " + file.length
                                + "\r\nConnection: close\r\n\r\n").getBytes(StandardCharsets.US_ASCII));
                        out.write(file);
                        out.flush();
                        if (listener != null) listener.downloaded(s.getInetAddress().getHostAddress());
                    } else {
                        out.write(("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: " + page.length
                                + "\r\nConnection: close\r\n\r\n").getBytes(StandardCharsets.US_ASCII));
                        out.write(page);
                        out.flush();
                    }
                } catch (IOException e) {
                    if (!running) return;
                }
            }
        }
    }

    /**
     * The page the PC's browser shows (assets/pc-setup.html), with this phone's name and the
     * file's size filled in. Falls back to the plain one if the asset can't be read.
     */
    static String page(Context c, String phone, long bytes) {
        try (InputStream in = c.getAssets().open("pc-setup.html")) {
            String html = new String(readAll(in), StandardCharsets.UTF_8);
            String size = String.format(java.util.Locale.ROOT, "%.1f MB", bytes / 1048576.0);
            return html.replace("{{PHONE}}", escape(phone)).replace("{{SIZE}}", size);
        } catch (IOException e) {
            return PAGE;
        }
    }

    static String escape(String s) {
        return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;").replace("\"", "&quot;");
    }

    /** A plain fallback page: one button. */
    static final String PAGE = "<!doctype html><html><head><meta charset=utf-8><meta name=viewport content='width=device-width'>"
            + "<title>DropCopy for this PC</title><style>"
            + "body{margin:0;min-height:100vh;display:grid;place-items:center;background:linear-gradient(#237EF0,#125AC8);"
            + "font-family:'Segoe UI Variable Display','Segoe UI',system-ui,sans-serif;color:#fff;text-align:center}"
            + "h1{font-size:34px;font-weight:600;margin:0 0 10px}p{opacity:.85;margin:0 0 28px;font-size:16px}"
            + "a{display:inline-block;background:#fff;color:#125AC8;text-decoration:none;font-weight:600;font-size:17px;"
            + "padding:14px 28px;border-radius:12px}small{display:block;margin-top:22px;opacity:.7;font-size:13px}"
            + "</style></head><body><div><h1>DropCopy for this PC</h1>"
            + "<p>Your phone is serving the PC app. It is already paired with your phone.</p>"
            + "<a href='/" + FILE + "'>Download DropCopy-Setup.exe</a>"
            + "<small>Then open it. If the browser warns about the download, choose Keep.</small></div></body></html>";

    private static byte[] readAll(InputStream in) throws IOException {
        ByteArrayOutputStream o = new ByteArrayOutputStream();
        byte[] buf = new byte[64 * 1024];
        for (int n; (n = in.read(buf)) > 0; ) o.write(buf, 0, n);
        return o.toByteArray();
    }
}
