package dev.dropcopy;

import android.content.Context;
import java.io.IOException;
import java.net.DatagramPacket;
import java.net.DatagramSocket;
import java.net.InetSocketAddress;
import java.net.ServerSocket;
import java.net.Socket;
import java.util.HashSet;
import java.util.Set;
import java.util.concurrent.atomic.AtomicInteger;

/**
 * Lets other phones send to this one: the same receiver a PC runs (TCP 48650, answers discovery
 * on UDP 48651), with this phone's own key. Runs only while something needs it: the screen is on
 * and a phone is paired (WatchService), or this phone's code is on screen (MainActivity).
 */
final class PhoneServer {
    private static final Set<String> holders = new HashSet<>();
    private static ServerSocket tcp;
    private static DatagramSocket udp;
    private static volatile int port = Proto.TCP_PORT;
    private static final AtomicInteger active = new AtomicInteger();
    /** Told when a phone that scanned this one's code says hello (the code's dialog closes). */
    static volatile java.util.function.Consumer<String> onPaired;

    private PhoneServer() {}

    /** The port it listens on (48650 unless something else on the phone has it). */
    static int port() {
        return port;
    }

    /** Starts it for {@code who}, if not already running. */
    static synchronized void hold(Context c, String who) {
        holders.add(who);
        if (tcp != null) return;
        Context app = c.getApplicationContext();
        try {
            ServerSocket s = new ServerSocket();
            s.setReuseAddress(true);
            try {
                s.bind(new InetSocketAddress(Proto.TCP_PORT));
            } catch (IOException taken) {
                s.bind(new InetSocketAddress(0));
            }
            tcp = s;
            port = s.getLocalPort();
            DatagramSocket u = new DatagramSocket(null);
            u.setReuseAddress(true);
            u.bind(new InetSocketAddress(Proto.UDP_PORT));
            udp = u;
        } catch (IOException e) {
            // UDP taken: it is still reachable at the address the other phone last saw.
        }
        ServerSocket s = tcp;
        DatagramSocket u = udp;
        if (s != null) new Thread(() -> accept(app, s), "phone-server").start();
        if (u != null) new Thread(() -> discovery(app, u), "phone-discovery").start();
    }

    static synchronized void release(String who) {
        holders.remove(who);
        if (!holders.isEmpty()) return;
        try { if (tcp != null) tcp.close(); } catch (IOException ignored) { }
        if (udp != null) udp.close();
        tcp = null;
        udp = null;
    }

    static synchronized boolean running() {
        return tcp != null;
    }

    private static void accept(Context c, ServerSocket s) {
        while (!s.isClosed()) {
            Socket conn;
            try {
                conn = s.accept();
            } catch (IOException e) {
                return; // closed by release()
            }
            if (active.incrementAndGet() > 4) {
                active.decrementAndGet();
                try { conn.close(); } catch (IOException ignored) { }
                continue;
            }
            new Thread(() -> {
                try (Socket x = conn) {
                    receive(c, x);
                } catch (IOException | RuntimeException ignored) {
                    // Wrong key, or the other phone gave up: nothing was kept.
                } finally {
                    active.decrementAndGet();
                }
            }, "phone-in").start();
        }
    }

    private static void receive(Context c, Socket conn) throws IOException {
        Proto.Pc me = Store.me(c);
        Receiver.Saver saver = new Receiver.Saver(c);
        Proto.Incoming got = Proto.serve(conn, me.key, saver);
        String from = got.device != null && !got.device.isEmpty() ? got.device : "a phone";
        if (got.pairLink != null) {
            try {
                Proto.Pc back = Proto.Pc.parse(got.pairLink);
                back.host = conn.getInetAddress().getHostAddress();
                boolean known = false;
                for (Proto.Pc p : Store.phones(c)) known |= p.hexId().equals(back.hexId());
                // Every send carries it: the sender's latest address, and a way to send back.
                Store.rememberPhone(c, back);
                from = back.name;
                java.util.function.Consumer<String> l = onPaired;
                if (l != null) l.accept(back.name);
                else if (!known) Transfer.toast(c, "Paired with " + back.name);
            } catch (IllegalArgumentException ignored) { }
        }
        if (got.count == 0) return;
        // A picture pours out of the top of the screen, then settles into the notification.
        boolean warp = false;
        for (int i = 0; i < saver.saved.size() && !warp; i++) {
            if (saver.mimes.get(i).startsWith("image/")) warp = WarpOverlay.show(c, saver.saved.get(i));
        }
        String who = from;
        if (warp) {
            new android.os.Handler(android.os.Looper.getMainLooper()).postDelayed(() -> saver.announce(who), Warp.RECEIVE_MS - 300);
        } else {
            saver.announce(who);
        }
    }

    /** Answers "where is phone <id>?" for this phone's id only. */
    private static void discovery(Context c, DatagramSocket u) {
        byte[] buf = new byte[64];
        Proto.Pc me = Store.me(c);
        while (!u.isClosed()) {
            DatagramPacket p = new DatagramPacket(buf, buf.length);
            try {
                u.receive(p);
            } catch (IOException e) {
                return;
            }
            byte[] reply = Proto.discoveryReply(Proto.slice(buf, 0, p.getLength()), me.id, me.key, port);
            if (reply == null) continue;
            try { u.send(new DatagramPacket(reply, reply.length, p.getAddress(), p.getPort())); } catch (IOException ignored) { }
        }
    }
}
