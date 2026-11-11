package dev.dropcopy;

import android.content.ContentUris;
import android.content.Context;
import android.database.Cursor;
import android.net.Uri;
import android.provider.MediaStore;
import java.io.IOException;
import java.net.DatagramPacket;
import java.net.DatagramSocket;
import java.net.InetAddress;
import java.net.SocketTimeoutException;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;

/**
 * While the screen is on, and "Show on PC when near" or "Receive from your PCs" is on: tells the
 * PCs where this phone is. When a PC says the phone just came close, sends it the newest photo as
 * a preview (the PC keeps it as a temporary file on the taskbar phone). When a PC says it has
 * something for this phone (an offer), collects it.
 */
class NearLink {
    private static final int HELLO_EVERY_MS = 15_000;
    /** The PC asks again each time the phone comes close; one preview per few seconds is plenty. */
    private static final int MIN_GAP_MS = 5_000;

    private static final int OFFER_GAP_MS = 3_000;
    private final java.util.Map<String, Long> lastOffer = new java.util.HashMap<>();
    private final Context ctx;
    private volatile DatagramSocket sock;

    NearLink(Context ctx) {
        this.ctx = ctx.getApplicationContext();
    }

    synchronized void start() {
        if (sock != null) return;
        try {
            sock = new DatagramSocket();
            sock.setBroadcast(true);
            sock.setSoTimeout(1000);
        } catch (IOException e) {
            sock = null;
            return;
        }
        DatagramSocket s = sock;
        new Thread(() -> run(s), "near-link").start();
    }

    boolean running() {
        return sock != null;
    }

    synchronized void stop() {
        DatagramSocket s = sock;
        sock = null;
        if (s != null) s.close();
    }

    private void run(DatagramSocket s) {
        long nextHello = 0, lastSent = 0;
        byte[] lastNonce = null;
        byte[] buf = new byte[64];
        while (sock == s) {
            List<Proto.Pc> pcs = Store.pcs(ctx);
            if (pcs.isEmpty()) {
                sleep(2000);
                continue;
            }
            Proto.Pc pc = pcs.get(0);
            long now = System.currentTimeMillis();
            if (now >= nextHello) {
                // Proximity is about the last PC used; receiving is from any of them.
                List<Proto.Pc> greet = Store.receive(ctx) ? pcs : pcs.subList(0, 1);
                List<InetAddress> broadcast = Proto.broadcastAddresses();
                for (Proto.Pc each : greet) {
                    byte[] hello = Proto.helloPacket(each);
                    List<InetAddress> to = new ArrayList<>();
                    try { if (each.host != null && !each.host.isEmpty()) to.add(InetAddress.getByName(each.host)); } catch (IOException ignored) { }
                    to.addAll(broadcast);
                    for (InetAddress a : to) {
                        try { s.send(new DatagramPacket(hello, hello.length, a, Proto.UDP_PORT)); } catch (IOException ignored) { }
                    }
                }
                nextHello = now + HELLO_EVERY_MS;
            }
            DatagramPacket p = new DatagramPacket(buf, buf.length);
            try {
                // Sleep in the socket until a pull arrives or the next hello is due: no polling.
                s.setSoTimeout((int) Math.max(50, nextHello - System.currentTimeMillis()));
                s.receive(p);
            } catch (SocketTimeoutException e) {
                continue;
            } catch (IOException e) {
                return; // closed by stop()
            }
            byte[] pkt = Proto.slice(buf, 0, p.getLength());
            if (Store.receive(ctx)) {
                for (Proto.Pc each : pcs) {
                    if (!Proto.isOffer(pkt, each)) continue;
                    // The PC sends each knock twice and repeats it while it waits: collect once.
                    Long last = lastOffer.get(each.hexId());
                    if (last != null && System.currentTimeMillis() - last < OFFER_GAP_MS) break;
                    lastOffer.put(each.hexId(), System.currentTimeMillis());
                    each.host = p.getAddress().getHostAddress();
                    SendService.fetch(ctx, each.hexId());
                    break;
                }
            }
            if (!Proto.isPull(pkt, pc)) continue;
            byte[] nonce = Proto.slice(pkt, 12, 28);
            if (Arrays.equals(nonce, lastNonce) || System.currentTimeMillis() - lastSent < MIN_GAP_MS) continue; // the PC sends it twice
            lastNonce = nonce;
            lastSent = System.currentTimeMillis();
            onPull(pc);
        }
    }

    /** The PC says the phone just came close. */
    void onPull(Proto.Pc pc) {
        Uri photo = newestImage(ctx);
        if (photo == null) return;
        try {
            List<Proto.Item> items = new ArrayList<>();
            items.add(Proto.options(Proto.OPT_NEARBY));
            items.add(Transfer.item(ctx, photo));
            Transfer.send(ctx, items, pc.hexId(), null);
        } catch (Exception ignored) {
            // Out of reach right now: the next time the phone comes close it tries again.
        }
    }

    /** The newest finished image on the phone, or null (none, or no permission to see photos). */
    static Uri newestImage(Context c) {
        Uri base = MediaStore.Images.Media.EXTERNAL_CONTENT_URI;
        try (Cursor cur = c.getContentResolver().query(base, new String[]{MediaStore.Images.Media._ID},
                MediaStore.Images.Media.IS_PENDING + " = 0", null, MediaStore.Images.Media.DATE_ADDED + " DESC")) {
            if (cur == null || !cur.moveToFirst()) return null;
            return ContentUris.withAppendedId(base, cur.getLong(0));
        } catch (RuntimeException e) {
            return null;
        }
    }

    private static void sleep(long ms) {
        try { Thread.sleep(ms); } catch (InterruptedException ignored) { }
    }
}
