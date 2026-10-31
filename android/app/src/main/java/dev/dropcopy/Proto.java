package dev.dropcopy;

import java.io.ByteArrayOutputStream;
import java.io.DataInputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.DatagramPacket;
import java.net.DatagramSocket;
import java.net.Inet4Address;
import java.net.InetAddress;
import java.net.InetSocketAddress;
import java.net.InterfaceAddress;
import java.net.NetworkInterface;
import java.net.Socket;
import java.net.SocketTimeoutException;
import java.net.URLDecoder;
import java.nio.ByteBuffer;
import java.nio.charset.StandardCharsets;
import java.security.GeneralSecurityException;
import java.security.MessageDigest;
import java.security.SecureRandom;
import java.util.ArrayList;
import java.util.Base64;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ArrayBlockingQueue;
import java.util.concurrent.BlockingQueue;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicBoolean;
import javax.crypto.Cipher;
import javax.crypto.Mac;
import javax.crypto.spec.GCMParameterSpec;
import javax.crypto.spec.SecretKeySpec;

/**
 * The DropCopy wire protocol, sender side. Plain Java (no Android imports) so it runs and is
 * tested on a desktop JVM against the real receiver. Mirrors {@code pc/src/proto.rs}.
 */
public final class Proto {
    public static final int TCP_PORT = 48650;
    public static final int UDP_PORT = 48651;
    static final int CHUNK = 512 * 1024;
    static final int MAX_RECORD = 1 << 20;
    static final int TAG = 16;

    static final byte T_FILE = 0x01, T_DATA = 0x02, T_FILE_END = 0x03, T_TEXT = 0x04, T_END = 0x05;
    static final byte T_OK = 0x10, T_ERR = 0x11;
    static final byte T_OPTS = 0x06;
    /** {@link #options} flags: a proximity preview (temporary), and "delete from the PC after pasting". */
    public static final int OPT_NEARBY = 0x01, OPT_DELETE_AFTER_PASTE = 0x02;
    static final byte[] HELLO = ascii("DCH1"), PULL = ascii("DCU1"),
            L_HELLO = ascii("dropcopy/v1/hello"), L_PULL = ascii("dropcopy/v1/pull");

    static final byte[] MAGIC = ascii("DCP1"), DISCOVER = ascii("DCD1"), REPLY = ascii("DCR1");
    static final byte[] PAIR_ASK = ascii("DCQ1"), PAIR_ANSWER = ascii("DCA1");
    static final byte[] NEAR_ASK = ascii("DCN1"), NEAR_ANSWER = ascii("DCM1"), L_NEARQ = ascii("dropcopy/v1/nearq");
    static final byte[] L_SRV = ascii("dropcopy/v1/srv"), L_C2S = ascii("dropcopy/v1/c2s"),
            L_S2C = ascii("dropcopy/v1/s2c"), L_DISC = ascii("dropcopy/v1/disc");

    private static final SecureRandom RNG = new SecureRandom();

    private Proto() {}

    /** A paired PC, as described by its pairing link. */
    public static final class Pc {
        public final byte[] id;
        public final byte[] key;
        public final String name;
        /** Where it was last reached; tried first, before any broadcast. */
        public volatile String host;
        public volatile int port;

        public Pc(byte[] id, byte[] key, String name, String host, int port) {
            this.id = id;
            this.key = key;
            this.name = name;
            this.host = host;
            this.port = port;
        }

        public String hexId() {
            StringBuilder sb = new StringBuilder();
            for (byte b : id) sb.append(String.format("%02x", b & 0xff));
            return sb.toString();
        }

        /** Serialises back to a pairing link carrying the latest address. */
        public String toLink() {
            StringBuilder sb = new StringBuilder("dropcopy://pair?v=1&id=").append(hexId())
                    .append("&k=").append(Base64.getUrlEncoder().withoutPadding().encodeToString(key))
                    .append("&n=").append(enc(name)).append("&p=").append(port);
            if (host != null && !host.isEmpty()) sb.append("&a=").append(host);
            return sb.toString();
        }

        /** Parses {@code dropcopy://pair?v=1&id=HEX16&k=B64URL&n=NAME&p=PORT&a=IP}. */
        public static Pc parse(String link) {
            if (link == null) throw new IllegalArgumentException("empty link");
            link = link.trim();
            int q = link.indexOf('?');
            if (!link.startsWith("dropcopy://pair") || q < 0) throw new IllegalArgumentException("not a DropCopy pairing link");
            Map<String, String> p = new LinkedHashMap<>();
            for (String kv : link.substring(q + 1).split("&")) {
                int e = kv.indexOf('=');
                if (e > 0) p.put(kv.substring(0, e), urlDecode(kv.substring(e + 1)));
            }
            byte[] id = unhex(p.get("id"));
            byte[] key = p.get("k") == null ? null : Base64.getUrlDecoder().decode(p.get("k"));
            if (id == null || id.length != 8 || key == null || key.length != 32) {
                throw new IllegalArgumentException("pairing link is incomplete");
            }
            String name = p.getOrDefault("n", "PC");
            int port = TCP_PORT;
            try { port = Integer.parseInt(p.getOrDefault("p", "" + TCP_PORT)); } catch (NumberFormatException ignored) { }
            String host = p.get("a");
            if (host != null && host.contains(",")) host = host.substring(0, host.indexOf(','));
            return new Pc(id, key, name, host, port);
        }
    }

    /** Something to send: a file-like stream, or text for the clipboard. */
    public interface Item {
        String name();
        String mime();
        /** Size in bytes, or -1 if unknown. */
        long size();
        InputStream open() throws IOException;
        /** Non-null for a text item. */
        default String text() { return null; }
        /** T_OPTS flags for an options item, else -1. */
        default int options() { return -1; }
    }

    /** Tells the PC how to treat the files that follow. */
    public static Item options(final int flags) {
        return new Item() {
            public String name() { return "options"; }
            public String mime() { return ""; }
            public long size() { return 0; }
            public InputStream open() { throw new UnsupportedOperationException(); }
            public int options() { return flags; }
        };
    }

    /** "DCH1" | id | nonce[16] | HMAC(K, hello | nonce)[..16]: where the PC can reach this phone. */
    static byte[] helloPacket(Pc pc) {
        byte[] nonce = random(16);
        ByteBuffer b = ByteBuffer.allocate(44);
        b.put(HELLO).put(pc.id).put(nonce).put(hmac(pc.key, L_HELLO, nonce), 0, 16);
        return b.array();
    }

    /** True for an authentic "send me your newest photo" from {@code pc}. */
    static boolean isPull(byte[] pkt, Pc pc) {
        if (pkt.length != 44) return false;
        for (int i = 0; i < 4; i++) if (pkt[i] != PULL[i]) return false;
        for (int i = 0; i < 8; i++) if (pkt[4 + i] != pc.id[i]) return false;
        return MessageDigest.isEqual(slice(hmac(pc.key, L_PULL, slice(pkt, 12, 28)), 0, 16), slice(pkt, 28, 44));
    }

    public static Item text(final String s) {
        return new Item() {
            public String name() { return "text"; }
            public String mime() { return "text/plain"; }
            public long size() { return s.length(); }
            public InputStream open() { throw new UnsupportedOperationException(); }
            public String text() { return s; }
        };
    }

    /**
     * A network path to try. On Android there is one per usable network (Wi-Fi, and the default
     * route); every path is raced, so a Wi-Fi without routes or without internet costs nothing.
     */
    public interface Route {
        Socket newSocket() throws IOException;
        void bind(DatagramSocket s) throws IOException;
    }

    /** The OS default route. */
    public static final Route DEFAULT_ROUTE = new Route() {
        public Socket newSocket() { return new Socket(); }
        public void bind(DatagramSocket s) { }
    };

    /** The PC could not be reached at all (as opposed to refusing this phone). Worth retrying later. */
    public static final class Unreachable extends IOException {
        public Unreachable(String msg) { super(msg); }
    }

    public interface Progress {
        void update(long sent, long total);
    }

    public static final class Result {
        public Pc pc;
        public String host;
        public long bytes;
        public long connectMillis;
        public long totalMillis;
    }

    // ---- proximity -------------------------------------------------------------------------

    /** Bluetooth LE manufacturer id 0xFFFF (reserved for testing) carries "DC" | token. */
    public static final int NEAR_COMPANY_ID = 0xFFFF;
    static final byte[] L_NEAR = ascii("dropcopy/v1/near");

    /** Changes every minute, so the advertisement means something only to the paired PC. */
    public static byte[] nearToken(byte[] key, long minute) {
        byte[] m = ByteBuffer.allocate(8).putLong(minute).array();
        return slice(hmac(key, L_NEAR, m), 0, 8);
    }

    public static byte[] nearPayload(byte[] key, long minute) {
        return ByteBuffer.allocate(10).put((byte) 'D').put((byte) 'C').put(nearToken(key, minute)).array();
    }

    // ---- crypto --------------------------------------------------------------------------

    static byte[] hmac(byte[] key, byte[]... parts) {
        try {
            Mac m = Mac.getInstance("HmacSHA256");
            m.init(new SecretKeySpec(key, "HmacSHA256"));
            for (byte[] p : parts) m.update(p);
            return m.doFinal();
        } catch (GeneralSecurityException e) {
            throw new IllegalStateException(e);
        }
    }

    static byte[] random(int n) {
        byte[] b = new byte[n];
        RNG.nextBytes(b);
        return b;
    }

    /** One direction of the record stream: u32 length | AES-256-GCM(type | body). */
    static final class Records {
        private final SecretKeySpec key;
        private final Cipher cipher;
        private long ctr;
        private byte[] out = new byte[0];

        Records(byte[] k) throws IOException {
            try {
                key = new SecretKeySpec(k, "AES");
                cipher = Cipher.getInstance("AES/GCM/NoPadding");
            } catch (GeneralSecurityException e) {
                throw new IOException(e);
            }
        }

        private GCMParameterSpec nextNonce() {
            byte[] n = ByteBuffer.allocate(12).putInt(0).putLong(ctr++).array();
            return new GCMParameterSpec(128, n);
        }

        /** {@code plain[0]} is the record type, {@code plain[1..len)} the body. */
        void write(OutputStream os, byte[] plain, int len) throws IOException {
            if (len > MAX_RECORD) throw new IOException("record too large");
            if (out.length < 4 + len + TAG) out = new byte[4 + len + TAG];
            try {
                cipher.init(Cipher.ENCRYPT_MODE, key, nextNonce());
                int n = cipher.doFinal(plain, 0, len, out, 4);
                out[0] = (byte) (n >>> 24); out[1] = (byte) (n >>> 16); out[2] = (byte) (n >>> 8); out[3] = (byte) n;
                os.write(out, 0, 4 + n);
            } catch (GeneralSecurityException e) {
                throw new IOException("encrypt failed", e);
            }
        }

        void write(OutputStream os, byte type, byte[] body) throws IOException {
            byte[] p = new byte[1 + body.length];
            p[0] = type;
            System.arraycopy(body, 0, p, 1, body.length);
            write(os, p, p.length);
        }

        /** Returns type byte followed by body. */
        byte[] read(DataInputStream in) throws IOException {
            int len = in.readInt();
            if (len < 1 + TAG || len > MAX_RECORD + TAG) throw new IOException("bad record length");
            byte[] ct = new byte[len];
            in.readFully(ct);
            try {
                cipher.init(Cipher.DECRYPT_MODE, key, nextNonce());
                return cipher.doFinal(ct);
            } catch (GeneralSecurityException e) {
                throw new IOException("reply failed authentication", e);
            }
        }
    }

    // ---- finding the PC --------------------------------------------------------------------

    /** Every IPv4 broadcast address of every interface that is up, plus 255.255.255.255. */
    public static List<InetAddress> broadcastAddresses() {
        List<InetAddress> out = new ArrayList<>();
        try {
            for (NetworkInterface nif : Collections.list(NetworkInterface.getNetworkInterfaces())) {
                if (!nif.isUp() || nif.isLoopback()) continue;
                for (InterfaceAddress a : nif.getInterfaceAddresses()) {
                    if (a.getAddress() instanceof Inet4Address && a.getBroadcast() != null && !out.contains(a.getBroadcast())) {
                        out.add(a.getBroadcast());
                    }
                }
            }
        } catch (IOException ignored) { }
        try { out.add(InetAddress.getByName("255.255.255.255")); } catch (IOException ignored) { }
        return out;
    }

    /**
     * Asks the network whether a DropCopy PC is here. Any PC that hears it puts its pairing code
     * on screen, and answers with its name (nothing secret: the key only travels in the QR code).
     * Returns the first name, or null if none answered within {@code timeoutMs}.
     */
    public static String askToPair(List<InetAddress> broadcasts, int timeoutMs) {
        byte[] nonce = random(8);
        byte[] ask = new byte[12];
        System.arraycopy(PAIR_ASK, 0, ask, 0, 4);
        System.arraycopy(nonce, 0, ask, 4, 8);
        long deadline = System.currentTimeMillis() + timeoutMs;
        try (DatagramSocket udp = new DatagramSocket()) {
            udp.setBroadcast(true);
            byte[] buf = new byte[128];
            int sent = 0;
            while (System.currentTimeMillis() < deadline) {
                if (sent < 3) { // a few tries: Wi-Fi drops broadcasts
                    for (InetAddress b : broadcasts) {
                        try { udp.send(new DatagramPacket(ask, ask.length, b, UDP_PORT)); } catch (IOException ignored) { }
                    }
                    sent++;
                }
                udp.setSoTimeout((int) Math.max(1, Math.min(300, deadline - System.currentTimeMillis())));
                DatagramPacket p = new DatagramPacket(buf, buf.length);
                try { udp.receive(p); } catch (SocketTimeoutException e) { continue; }
                String name = parsePairAnswer(slice(buf, 0, p.getLength()), nonce);
                if (name != null) return name;
            }
        } catch (IOException ignored) { }
        return null;
    }

    /** What the PC's Bluetooth watcher reports about this phone. */
    public static final class NearStatus {
        public static final int OFF = 0, NO_BLUETOOTH = 1, SCANNING = 2;
        public final int state;
        /** Smoothed strength in dBm, or null if not heard in the last few seconds. */
        public final Integer dbm;
        /** How strong counts as close. */
        public final int threshold;

        NearStatus(int state, Integer dbm, int threshold) {
            this.state = state;
            this.dbm = dbm;
            this.threshold = threshold;
        }
    }

    /** Asks the PC whether it hears this phone over Bluetooth; null if it doesn't answer. */
    public static NearStatus askNear(Pc pc, List<InetAddress> targets, int timeoutMs) {
        byte[] nonce = random(16);
        byte[] ask = new byte[28];
        System.arraycopy(NEAR_ASK, 0, ask, 0, 4);
        System.arraycopy(pc.id, 0, ask, 4, 8);
        System.arraycopy(nonce, 0, ask, 12, 16);
        List<InetAddress> to = new ArrayList<>();
        try { if (pc.host != null && !pc.host.isEmpty()) to.add(InetAddress.getByName(pc.host)); } catch (IOException ignored) { }
        to.addAll(targets);
        long deadline = System.currentTimeMillis() + timeoutMs;
        try (DatagramSocket udp = new DatagramSocket()) {
            udp.setBroadcast(true);
            for (InetAddress a : to) {
                try { udp.send(new DatagramPacket(ask, ask.length, a, UDP_PORT)); } catch (IOException ignored) { }
            }
            byte[] buf = new byte[64];
            while (System.currentTimeMillis() < deadline) {
                udp.setSoTimeout((int) Math.max(1, deadline - System.currentTimeMillis()));
                DatagramPacket p = new DatagramPacket(buf, buf.length);
                try { udp.receive(p); } catch (SocketTimeoutException e) { break; }
                NearStatus st = parseNearAnswer(slice(buf, 0, p.getLength()), pc, nonce);
                if (st != null) return st;
            }
        } catch (IOException ignored) { }
        return null;
    }

    static NearStatus parseNearAnswer(byte[] pkt, Pc pc, byte[] nonce) {
        if (pkt.length != 35) return null;
        for (int i = 0; i < 4; i++) if (pkt[i] != NEAR_ANSWER[i]) return null;
        for (int i = 0; i < 8; i++) if (pkt[4 + i] != pc.id[i]) return null;
        byte[] body = slice(pkt, 12, 19);
        byte[] want = slice(hmac(pc.key, L_NEARQ, nonce, body), 0, 16);
        if (!MessageDigest.isEqual(want, slice(pkt, 19, 35))) return null;
        ByteBuffer b = ByteBuffer.wrap(body);
        int state = b.get() & 0xff;
        short dbm = b.getShort();
        short threshold = b.getShort();
        return new NearStatus(state, dbm == Short.MIN_VALUE ? null : (int) dbm, threshold);
    }

    /** "DCA1" | nonce[8] | name; null if it isn't the answer to {@code nonce}. */
    static String parsePairAnswer(byte[] pkt, byte[] nonce) {
        if (pkt.length < 12 || pkt.length > 12 + 64) return null;
        for (int i = 0; i < 4; i++) if (pkt[i] != PAIR_ANSWER[i]) return null;
        for (int i = 0; i < 8; i++) if (pkt[4 + i] != nonce[i]) return null;
        String name = new String(pkt, 12, pkt.length - 12, StandardCharsets.UTF_8).trim();
        return name.isEmpty() ? "your PC" : name;
    }

    static final class Conn {
        final Socket socket;
        final Pc pc;
        final String host;
        final int port;

        Conn(Socket socket, Pc pc, String host, int port) {
            this.socket = socket;
            this.pc = pc;
            this.host = host;
            this.port = port;
        }
    }

    /**
     * Races a direct connect to every PC's last known address against a broadcast discovery,
     * and returns the first connection that opens. On an unchanged network the direct connect
     * wins in a few milliseconds; after the PC's address changes, discovery finds it.
     */
    static Conn connect(final List<Pc> pcs, final List<InetAddress> broadcasts, List<Route> routes, int timeoutMs) throws IOException {
        final BlockingQueue<Object> results = new ArrayBlockingQueue<>(256);
        final AtomicBoolean done = new AtomicBoolean(false);
        final List<Thread> threads = new ArrayList<>();
        final List<DatagramSocket> udps = new ArrayList<>();

        for (final Route route : routes) {
            for (final Pc pc : pcs) {
                if (pc.host == null || pc.host.isEmpty()) continue;
                threads.add(new Thread(() -> {
                    try {
                        results.offer(open(route, pc, pc.host, pc.port, 1500, done));
                    } catch (IOException e) {
                        results.offer(e);
                    }
                }, "direct-" + pc.name));
            }
            final DatagramSocket udp;
            try {
                udp = new DatagramSocket();
                route.bind(udp);
                udp.setBroadcast(true);
            } catch (IOException e) {
                results.offer(e);
                continue;
            }
            udps.add(udp);
            threads.add(new Thread(() -> {
                try {
                    discover(route, udp, pcs, broadcasts, done, results);
                } catch (IOException e) {
                    results.offer(e);
                }
            }, "discover"));
        }

        for (Thread t : threads) { t.setDaemon(true); t.start(); }
        long deadline = System.currentTimeMillis() + timeoutMs;
        IOException last = null;
        try {
            while (true) {
                long left = deadline - System.currentTimeMillis();
                Object r = left > 0 ? results.poll(left, TimeUnit.MILLISECONDS) : null;
                if (r == null) break;
                if (r instanceof Conn) return (Conn) r;
                if (r instanceof IOException) last = (IOException) r;
            }
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
        } finally {
            done.set(true);
            for (DatagramSocket u : udps) u.close();
            // A late winner is closed by open() itself once done is set.
            for (Object r; (r = results.poll()) != null; ) {
                if (r instanceof Conn) try { ((Conn) r).socket.close(); } catch (IOException ignored) { }
            }
        }
        String names = pcs.size() == 1 ? pcs.get(0).name : "your PC";
        throw new Unreachable("Couldn't find " + names + " on this network"
                + (last != null && last.getMessage() != null ? " (" + last.getMessage() + ")" : ""));
    }

    private static Conn open(Route route, Pc pc, String host, int port, int timeoutMs, AtomicBoolean done) throws IOException {
        Socket s = route.newSocket();
        try {
            s.setTcpNoDelay(true);
            s.connect(new InetSocketAddress(host, port), timeoutMs);
        } catch (IOException e) {
            s.close();
            throw e;
        }
        if (done.get()) {
            s.close();
            throw new IOException("lost the race");
        }
        return new Conn(s, pc, host, port);
    }

    private static void discover(Route route, DatagramSocket udp, List<Pc> pcs, List<InetAddress> broadcasts,
                                 AtomicBoolean done, BlockingQueue<Object> results) throws IOException {
        byte[] nonce = random(16);
        List<byte[]> packets = new ArrayList<>();
        for (Pc pc : pcs) {
            packets.add(ByteBuffer.allocate(28).put(DISCOVER).put(pc.id).put(nonce).array());
        }
        byte[] buf = new byte[64];
        java.util.Set<String> started = new java.util.HashSet<>();
        long nextSend = 0;
        int interval = 100;
        while (!done.get()) {
            long now = System.currentTimeMillis();
            if (now >= nextSend) {
                for (InetAddress b : broadcasts) {
                    for (byte[] p : packets) {
                        try { udp.send(new DatagramPacket(p, p.length, b, UDP_PORT)); } catch (IOException ignored) { }
                    }
                }
                nextSend = now + interval;
                interval = Math.min(interval * 2, 800);
            }
            udp.setSoTimeout((int) Math.max(5, nextSend - System.currentTimeMillis()));
            DatagramPacket in = new DatagramPacket(buf, buf.length);
            try {
                udp.receive(in);
            } catch (SocketTimeoutException e) {
                continue;
            } catch (IOException e) {
                if (done.get()) return;
                throw e;
            }
            if (in.getLength() != 30) continue;
            ByteBuffer r = ByteBuffer.wrap(buf, 0, 30);
            byte[] magic = new byte[4], id = new byte[8], tag = new byte[16];
            r.get(magic).get(id);
            int port = r.getShort() & 0xffff;
            r.get(tag);
            if (!MessageDigest.isEqual(magic, REPLY)) continue;
            for (final Pc pc : pcs) {
                if (!MessageDigest.isEqual(id, pc.id)) continue;
                byte[] want = hmac(pc.key, L_DISC, nonce, new byte[]{(byte) (port >>> 8), (byte) port});
                byte[] want16 = new byte[16];
                System.arraycopy(want, 0, want16, 0, 16);
                if (!MessageDigest.isEqual(tag, want16)) continue;
                final String host = in.getAddress().getHostAddress();
                final int p = port;
                if (!started.add(pc.hexId() + "@" + host + ":" + p)) continue; // replies to repeated broadcasts
                Thread t = new Thread(() -> {
                    try { results.offer(open(route, pc, host, p, 1500, done)); } catch (IOException e) { results.offer(e); }
                }, "found-" + pc.name);
                t.setDaemon(true);
                t.start();
            }
        }
    }

    // ---- sending ---------------------------------------------------------------------------

    /** Finds a PC from {@code pcs}, sends {@code items}, and waits for the PC to confirm. */
    public static Result send(List<Pc> pcs, List<InetAddress> broadcasts, List<Item> items, Progress progress) throws IOException {
        return send(pcs, broadcasts, Collections.singletonList(DEFAULT_ROUTE), items, progress);
    }

    public static Result send(List<Pc> pcs, List<InetAddress> broadcasts, List<Route> routes, List<Item> items, Progress progress) throws IOException {
        if (pcs.isEmpty()) throw new IOException("No PC paired yet");
        long t0 = System.currentTimeMillis();
        Conn c = connect(pcs, broadcasts, routes, 4000);
        Result res = new Result();
        res.pc = c.pc;
        res.host = c.host;
        res.connectMillis = System.currentTimeMillis() - t0;
        c.pc.host = c.host;
        c.pc.port = c.port;
        try (Socket s = c.socket) {
            s.setSoTimeout(60_000);
            s.setSendBufferSize(1 << 20);
            DataInputStream in = new DataInputStream(new java.io.BufferedInputStream(s.getInputStream()));
            OutputStream out = new java.io.BufferedOutputStream(s.getOutputStream(), CHUNK + 64);

            // Handshake: hello, then the PC proves it holds the key.
            byte[] cn = random(16);
            out.write(MAGIC);
            out.write(cn);
            out.flush();
            byte[] resp = new byte[52];
            in.readFully(resp);
            byte[] sn = slice(resp, 4, 20);
            if (!MessageDigest.isEqual(slice(resp, 0, 4), MAGIC)
                    || !MessageDigest.isEqual(slice(resp, 20, 52), hmac(c.pc.key, L_SRV, cn, sn))) {
                throw new IOException(c.pc.name + " did not recognise this phone - pair again");
            }
            Records tx = new Records(hmac(c.pc.key, L_C2S, cn, sn));
            Records rx = new Records(hmac(c.pc.key, L_S2C, cn, sn));

            long total = 0;
            for (Item it : items) total += Math.max(0, it.size());
            byte[] plain = new byte[CHUNK];
            for (Item it : items) {
                if (it.options() >= 0) {
                    tx.write(out, T_OPTS, new byte[]{(byte) it.options()});
                    continue;
                }
                if (it.text() != null) {
                    tx.write(out, T_TEXT, it.text().getBytes(StandardCharsets.UTF_8));
                    continue;
                }
                tx.write(out, T_FILE, fileHeader(it.size(), it.name(), it.mime()));
                try (InputStream is = it.open()) {
                    while (true) {
                        int n = readUpTo(is, plain, 1, CHUNK - 1);
                        if (n <= 0) break;
                        plain[0] = T_DATA;
                        tx.write(out, plain, 1 + n);
                        res.bytes += n;
                        if (progress != null) progress.update(res.bytes, total);
                    }
                }
                tx.write(out, T_FILE_END, new byte[0]);
            }
            tx.write(out, T_END, new byte[0]);
            out.flush();

            byte[] reply = rx.read(in);
            if (reply[0] == T_ERR) {
                throw new IOException(c.pc.name + ": " + new String(reply, 1, reply.length - 1, StandardCharsets.UTF_8));
            }
            if (reply[0] != T_OK) throw new IOException("unexpected reply from " + c.pc.name);
        }
        res.totalMillis = System.currentTimeMillis() - t0;
        return res;
    }

    // ---- helpers -----------------------------------------------------------------------------

    private static int readUpTo(InputStream is, byte[] b, int off, int len) throws IOException {
        int got = 0;
        while (got < len) {
            int n = is.read(b, off + got, len - got);
            if (n < 0) break;
            got += n;
        }
        return got;
    }

    static byte[] fileHeader(long size, String name, String mime) {
        byte[] n = utf8Max(name == null ? "file" : name), m = utf8Max(mime == null ? "application/octet-stream" : mime);
        return ByteBuffer.allocate(12 + n.length + m.length)
                .putLong(size < 0 ? -1L : size)
                .putShort((short) n.length).put(n)
                .putShort((short) m.length).put(m)
                .array();
    }

    private static byte[] utf8Max(String s) {
        byte[] b = s.getBytes(StandardCharsets.UTF_8);
        return b.length <= 4096 ? b : slice(b, 0, 4096);
    }

    static byte[] slice(byte[] b, int from, int to) {
        byte[] r = new byte[to - from];
        System.arraycopy(b, from, r, 0, r.length);
        return r;
    }

    static byte[] ascii(String s) {
        return s.getBytes(StandardCharsets.US_ASCII);
    }

    static byte[] unhex(String s) {
        if (s == null || s.length() % 2 != 0) return null;
        ByteArrayOutputStream o = new ByteArrayOutputStream();
        for (int i = 0; i < s.length(); i += 2) {
            int hi = Character.digit(s.charAt(i), 16), lo = Character.digit(s.charAt(i + 1), 16);
            if (hi < 0 || lo < 0) return null;
            o.write(hi * 16 + lo);
        }
        return o.toByteArray();
    }

    static String urlDecode(String s) {
        try {
            // URLDecoder turns '+' into a space; the PC never sends a raw '+', so keep it literal.
            return URLDecoder.decode(s.replace("+", "%2B"), "UTF-8");
        } catch (java.io.UnsupportedEncodingException | IllegalArgumentException e) {
            return s;
        }
    }

    static String enc(String s) {
        StringBuilder sb = new StringBuilder();
        for (byte b : s.getBytes(StandardCharsets.UTF_8)) {
            int c = b & 0xff;
            if ((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || "-._~".indexOf(c) >= 0) sb.append((char) c);
            else sb.append(String.format("%%%02X", c));
        }
        return sb.toString();
    }
}
