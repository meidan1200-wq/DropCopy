package dev.dropcopy;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.DatagramPacket;
import java.net.DatagramSocket;
import java.net.InetAddress;
import java.net.InetSocketAddress;
import java.net.ServerSocket;
import java.net.Socket;
import java.util.ArrayList;
import java.util.Base64;
import java.util.List;
import java.util.Random;
import java.util.concurrent.atomic.AtomicReference;

/**
 * Phone to phone on a desktop JVM: one phone's receiver ({@link Proto#serve}, discovery answers)
 * and the other phone's sender ({@link Proto#send}), over real sockets. Also the warp's geometry.
 *
 *   javac -d out ../app/src/main/java/dev/dropcopy/Proto.java ../app/src/main/java/dev/dropcopy/Warp.java PhoneTest.java
 *   java -cp out dev.dropcopy.PhoneTest
 */
public class PhoneTest {
    static int failures = 0;

    static void check(String what, boolean ok) {
        System.out.println((ok ? "  PASS " : "  FAIL ") + what);
        if (!ok) failures++;
    }

    static final class Got implements Proto.Sink {
        final List<String> names = new ArrayList<>(), texts = new ArrayList<>();
        final List<byte[]> files = new ArrayList<>();
        ByteArrayOutputStream cur;
        int aborted;

        public OutputStream begin(String name, String mime, long size) {
            names.add(name + "|" + mime + "|" + size);
            return cur = new ByteArrayOutputStream();
        }
        public void end() { files.add(cur.toByteArray()); }
        public void abort() { aborted++; }
        public void text(String t) { texts.add(t); }
    }

    /** Receives one transfer on {@code server} with {@code key}. */
    static Thread receiver(ServerSocket server, byte[] key, Got sink, AtomicReference<Object> out) {
        Thread t = new Thread(() -> {
            try (Socket s = server.accept()) {
                out.set(Proto.serve(s, key, sink));
            } catch (IOException e) {
                out.set(e);
            }
        });
        t.start();
        return t;
    }

    static Proto.Item file(String name, String mime, byte[] data) {
        return new Proto.Item() {
            public String name() { return name; }
            public String mime() { return mime; }
            public long size() { return data.length; }
            public InputStream open() { return new java.io.ByteArrayInputStream(data); }
        };
    }

    public static void main(String[] args) throws Exception {
        byte[] key = new byte[32], id = {1, 2, 3, 4, 5, 6, 7, 8};
        new Random(7).nextBytes(key);
        String k = Base64.getUrlEncoder().withoutPadding().encodeToString(key);
        List<Proto.Route> route = List.of(Proto.DEFAULT_ROUTE);

        try (ServerSocket server = new ServerSocket(0, 4, InetAddress.getByName("127.0.0.1"))) {
            int port = server.getLocalPort();
            Proto.Pc phoneB = Proto.Pc.parse("dropcopy://pair?v=1&id=0102030405060708&k=" + k + "&n=Galaxy%20S26&p=" + port + "&a=127.0.0.1&t=phone");
            check("a phone's code says it is a phone", phoneB.phone && phoneB.toLink().contains("&t=phone"));

            // 1. Phone A scanned B's code: a hello with its own link, then a photo and a text.
            byte[] photo = new byte[1_300_000];
            new Random(3).nextBytes(photo);
            String aLink = "dropcopy://pair?v=1&id=a1a2a3a4a5a6a7a8&k=" + k + "&n=Pixel%209&p=48650&a=10.0.0.4&t=phone";
            Got got = new Got();
            AtomicReference<Object> in = new AtomicReference<>();
            Thread t = receiver(server, key, got, in);
            Proto.Result r = Proto.send(List.of(phoneB), List.of(), route,
                    List.of(Proto.device("Pixel 9", "a1"), Proto.pairBack(aLink), file("beach.jpg", "image/jpeg", photo), Proto.text("look")), null);
            t.join(10_000);
            Proto.Incoming inc = in.get() instanceof Proto.Incoming ? (Proto.Incoming) in.get() : null;
            check("B received it all: " + in.get(), inc != null && inc.count == 2 && got.files.size() == 1
                    && java.util.Arrays.equals(got.files.get(0), photo) && got.texts.equals(List.of("look")));
            check("with the name, type and size", got.names.equals(List.of("beach.jpg|image/jpeg|1300000")));
            check("B learned who sent it and how to send back", inc != null && "Pixel 9".equals(inc.device) && aLink.equals(inc.pairLink));
            check("B's OK counts 2 and says v2", r.bytes == photo.length && (r.flags & Proto.OK_V2) != 0 && phoneB.v2);

            // 2. A second send now also carries A's install id (v2): B takes it in its stride.
            got = new Got();
            t = receiver(server, key, got, in);
            Proto.send(List.of(phoneB), List.of(), route, List.of(Proto.device("Pixel 9", "a1"), Proto.text("again")), null);
            t.join(10_000);
            check("a later send with the install id works", got.texts.equals(List.of("again")));

            // 3. Someone without B's key: refused before anything is saved.
            byte[] wrong = key.clone();
            wrong[0] ^= 1;
            Proto.Pc impostor = new Proto.Pc(id, wrong, "Galaxy S26", "127.0.0.1", port);
            impostor.phone = true;
            got = new Got();
            t = receiver(server, key, got, in);
            String err = null;
            try {
                Proto.send(List.of(impostor), List.of(), route, List.of(Proto.text("let me in")), null);
            } catch (IOException e) {
                err = e.getMessage();
            }
            t.join(10_000);
            check("a wrong key is refused: " + err, err != null && got.texts.isEmpty() && got.files.isEmpty());

            // 4. The pair-back record goes only to phones: a PC (older ones refuse unknown records) never sees it.
            Proto.Pc asPc = new Proto.Pc(id, key, "PC", "127.0.0.1", port);
            got = new Got();
            t = receiver(server, key, got, in);
            Proto.send(List.of(asPc), List.of(), route, List.of(Proto.pairBack(aLink), Proto.text("pc")), null);
            t.join(10_000);
            Proto.Incoming pcIn = (Proto.Incoming) in.get();
            check("no pair-back to a PC", pcIn.pairLink == null && got.texts.equals(List.of("pc")));

            // 5. B moved to another address: A finds it by asking the network, as it finds a PC.
            DatagramSocket disc = null;
            try {
                disc = new DatagramSocket(null);
                disc.setReuseAddress(true);
                disc.bind(new InetSocketAddress(InetAddress.getByName("127.0.0.1"), Proto.UDP_PORT));
            } catch (IOException e) {
                System.out.println("  SKIP discovery: UDP " + Proto.UDP_PORT + " is taken here");
            }
            if (disc != null) {
                DatagramSocket u = disc;
                Thread answer = new Thread(() -> {
                    byte[] b = new byte[64];
                    try {
                        for (int i = 0; i < 20; i++) {
                            DatagramPacket p = new DatagramPacket(b, b.length);
                            u.receive(p);
                            byte[] rep = Proto.discoveryReply(Proto.slice(b, 0, p.getLength()), id, key, port);
                            if (rep != null) u.send(new DatagramPacket(rep, rep.length, p.getAddress(), p.getPort()));
                        }
                    } catch (IOException ignored) { }
                });
                answer.start();
                phoneB.host = null;
                got = new Got();
                t = receiver(server, key, got, in);
                Proto.send(List.of(phoneB), List.of(InetAddress.getByName("127.0.0.1")), route, List.of(Proto.text("found you")), null);
                t.join(10_000);
                check("found by discovery", got.texts.equals(List.of("found you")) && "127.0.0.1".equals(phoneB.host));
                byte[] other = new byte[28];
                System.arraycopy(Proto.DISCOVER, 0, other, 0, 4);
                check("asked for another id: no answer", Proto.discoveryReply(other, id, key, port) == null);
                disc.close();
            }
        }

        // 6. The warp: whole card at the start, all of it in the point at the end, rows near the point first.
        float[] v = new float[(Warp.COLS + 1) * (Warp.ROWS + 1) * 2];
        Warp.genie(0, 100, 200, 300, 500, 200, -8, 3, v);
        int last = v.length - 2;
        check("t=0: the card's corners", v[0] == 100 && v[1] == 200 && v[last] == 300 && v[last + 1] == 500);
        Warp.genie(1, 100, 200, 300, 500, 200, -8, 3, v);
        boolean in = true;
        for (int i = 0; i < v.length; i += 2) in &= Math.abs(v[i] - 200) <= 1.6f && Math.abs(v[i + 1] + 8) < 0.01f;
        check("t=1: everything in the point", in);
        Warp.genie(0.4f, 100, 200, 300, 500, 200, -8, 3, v);
        check("t=0.4: the top has travelled further than the bottom", (200 - v[1]) > (500 - v[last + 1]));
        Warp.Frame end = Warp.frame(true, Warp.SEND_MS, 1080, 2400, 4000, 3000, 2.75f, null);
        Warp.Frame rest = Warp.frame(false, 1200, 1080, 2400, 4000, 3000, 2.75f, null);
        check("sent: gone at the end", end.done && end.alpha < 0.01f);
        check("received: whole and still at rest", rest.alpha > 0.99f && !rest.done);

        System.out.println(failures == 0 ? "ALL PASSED" : failures + " FAILED");
        System.exit(failures == 0 ? 0 : 1);
    }
}
