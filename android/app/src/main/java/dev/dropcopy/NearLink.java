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
 * While "Show on PC when near" is on and the screen is on: tells the PC where this phone is, and
 * when the PC says the phone just came close, sends it the newest photo as a preview. The PC keeps
 * that preview as a temporary file and shows it on the taskbar phone; nothing else changes there.
 */
class NearLink {
    private static final int HELLO_EVERY_MS = 15_000;
    /** The PC asks again each time the phone comes close; one preview per few seconds is plenty. */
    private static final int MIN_GAP_MS = 5_000;

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
                byte[] hello = Proto.helloPacket(pc);
                List<InetAddress> to = new ArrayList<>();
                try { if (pc.host != null && !pc.host.isEmpty()) to.add(InetAddress.getByName(pc.host)); } catch (IOException ignored) { }
                to.addAll(Proto.broadcastAddresses());
                for (InetAddress a : to) {
                    try { s.send(new DatagramPacket(hello, hello.length, a, Proto.UDP_PORT)); } catch (IOException ignored) { }
                }
                nextHello = now + HELLO_EVERY_MS;
            }
            DatagramPacket p = new DatagramPacket(buf, buf.length);
            try {
                s.receive(p);
            } catch (SocketTimeoutException e) {
                continue;
            } catch (IOException e) {
                return; // closed by stop()
            }
            byte[] pkt = Proto.slice(buf, 0, p.getLength());
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
