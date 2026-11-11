package dev.dropcopy;

import static org.junit.Assert.assertArrayEquals;
import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertNull;
import static org.junit.Assert.assertTrue;
import static org.junit.Assume.assumeFalse;
import static org.robolectric.Shadows.shadowOf;

import android.app.Application;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Intent;
import android.content.pm.ShortcutInfo;
import android.content.pm.ShortcutManager;
import android.net.Uri;
import android.os.Looper;
import java.io.ByteArrayInputStream;
import java.io.File;
import java.nio.file.Files;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Random;
import org.junit.Test;
import org.junit.runner.RunWith;
import org.robolectric.Robolectric;
import org.robolectric.RobolectricTestRunner;
import org.robolectric.RuntimeEnvironment;
import org.robolectric.android.controller.ActivityController;
import org.robolectric.annotation.Config;

/** The Android glue, run on the real framework classes (Robolectric). */
@RunWith(RobolectricTestRunner.class)
@Config(sdk = 35)
public class AppTest {
    private static final String LINK = "dropcopy://pair?v=1&id=0011223344556677"
            + "&k=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8&n=My%20PC&p=48650&a=192.168.1.5";

    private final Application app = RuntimeEnvironment.getApplication();

    @Test
    public void askingToPairGetsThePcNameFromItsAnswer() throws Exception {
        java.net.DatagramSocket pc;
        try {
            pc = new java.net.DatagramSocket(Proto.UDP_PORT, java.net.InetAddress.getByName("127.0.0.1"));
        } catch (java.net.BindException e) {
            org.junit.Assume.assumeTrue("UDP port in use by a running receiver", false);
            return;
        }
        Thread t = new Thread(() -> {
            try {
                byte[] b = new byte[64];
                java.net.DatagramPacket p = new java.net.DatagramPacket(b, b.length);
                pc.receive(p);
                assertEquals(12, p.getLength());
                assertEquals("DCQ1", new String(b, 0, 4, "US-ASCII"));
                byte[] ans = new byte[12 + 15];
                System.arraycopy("DCA1".getBytes("US-ASCII"), 0, ans, 0, 4);
                System.arraycopy(b, 4, ans, 4, 8);
                System.arraycopy("DESKTOP-4E1E3TB".getBytes("UTF-8"), 0, ans, 12, 15);
                pc.send(new java.net.DatagramPacket(ans, ans.length, p.getSocketAddress()));
            } catch (Exception ignored) { }
        });
        t.start();
        try {
            String name = Proto.askToPair(Collections.singletonList(java.net.InetAddress.getByName("127.0.0.1")), 2000);
            assertEquals("DESKTOP-4E1E3TB", name);
        } finally {
            t.join(3000);
            pc.close();
        }
        // An answer to some other question is ignored.
        byte[] nonce = new byte[8];
        byte[] other = "DCA1\u0001\u0001\u0001\u0001\u0001\u0001\u0001\u0001PC".getBytes("ISO-8859-1");
        assertEquals(null, Proto.parsePairAnswer(other, nonce));
    }

    @Test
    public void pairedPhoneListsOnlyTheSettingsStillMissing() {
        Store.add(app, LINK);
        Store.setFlag(app, Store.WATCH_SCREENSHOTS, true);
        MainActivity a = Robolectric.buildActivity(MainActivity.class).setup().get();
        assertEquals(android.view.View.VISIBLE, a.findViewById(R.id.needs_card).getVisibility());
        assertEquals(android.view.View.VISIBLE, a.findViewById(R.id.row_ok_battery).getVisibility());
        // The first visible row has no divider above it.
        assertEquals(android.view.View.GONE, a.findViewById(R.id.row_ok_battery_div).getVisibility());

        // Battery allowed: that row goes; the card stays for what is still missing.
        org.robolectric.shadows.ShadowPowerManager pm = shadowOf(app.getSystemService(android.os.PowerManager.class));
        pm.setIgnoringBatteryOptimizations(app.getPackageName(), true);
        a.recreate();
        a = Robolectric.buildActivity(MainActivity.class).setup().get();
        assertEquals(android.view.View.GONE, a.findViewById(R.id.row_ok_battery).getVisibility());
    }

    @Test
    public void turningOnNearOpensSetupThatShowsWhatTheLaptopHears() throws Exception {
        java.net.DatagramSocket pcSock;
        try {
            pcSock = new java.net.DatagramSocket(Proto.UDP_PORT, java.net.InetAddress.getByName("127.0.0.1"));
        } catch (java.net.BindException e) {
            org.junit.Assume.assumeTrue("UDP port in use by a running receiver", false);
            return;
        }
        Store.add(app, LINK.replaceAll("&a=[^&]*", "") + "&a=127.0.0.1");
        Proto.Pc pc = Store.pcs(app).get(0);
        // A PC that hears the phone at -48 dBm and pops up at -55.
        Thread t = new Thread(() -> {
            try {
                byte[] b = new byte[64];
                for (int k = 0; k < 20; k++) {
                    java.net.DatagramPacket p = new java.net.DatagramPacket(b, b.length);
                    pcSock.receive(p);
                    if (p.getLength() != 28 || b[0] != 'D' || b[2] != 'N') continue;
                    byte[] nonce = java.util.Arrays.copyOfRange(b, 12, 28);
                    byte[] body = {2, (byte) 0xff, (byte) 0xd0, (byte) 0xff, (byte) 0xc9, 0, 100};
                    byte[] tag = Proto.hmac(pc.key, "dropcopy/v1/nearq".getBytes("US-ASCII"), nonce, body);
                    java.io.ByteArrayOutputStream o = new java.io.ByteArrayOutputStream();
                    o.write("DCM1".getBytes("US-ASCII"));
                    o.write(pc.id);
                    o.write(body);
                    o.write(tag, 0, 16);
                    byte[] ans = o.toByteArray();
                    pcSock.send(new java.net.DatagramPacket(ans, ans.length, p.getSocketAddress()));
                }
            } catch (Exception ignored) { }
        });
        t.start();
        try {
            shadowOf(app).grantPermissions(android.Manifest.permission.BLUETOOTH_ADVERTISE);
            android.bluetooth.BluetoothManager bm = app.getSystemService(android.bluetooth.BluetoothManager.class);
            shadowOf(bm.getAdapter()).setEnabled(true);
            MainActivity a = Robolectric.buildActivity(MainActivity.class).setup().get();
            a.findViewById(R.id.row_near).performClick();
            android.app.Dialog d = org.robolectric.shadows.ShadowDialog.getLatestDialog();
            assertNotNull("tapping the switch opens the setup window", d);
            android.widget.TextView seen = d.findViewById(R.id.near_seen_detail);
            for (int k = 0; k < 60 && !seen.getText().toString().contains("dBm"); k++) {
                Thread.sleep(50);
                shadowOf(Looper.getMainLooper()).idle();
            }
            assertEquals("Sees this phone · -48 dBm. Close enough: your newest photo pops up.", seen.getText().toString());
            assertEquals("Allowed", ((android.widget.TextView) d.findViewById(R.id.near_perm_detail)).getText().toString());
        } finally {
            pcSock.close();
            t.join(2000);
        }
    }

    @Test
    public void deleteAfterPasteNeedsTwoYesesAndCancelLeavesItOff() {
        Store.add(app, LINK);
        MainActivity a = Robolectric.buildActivity(MainActivity.class).setup().get();
        android.widget.Switch sw = a.findViewById(R.id.sw_delete);

        a.findViewById(R.id.row_delete).performClick();
        android.app.AlertDialog first = (android.app.AlertDialog) org.robolectric.shadows.ShadowDialog.getLatestDialog();
        assertTrue(shadowOf(first).getMessage().toString().contains("copy something else"));
        assertEquals(false, sw.isChecked());
        first.getButton(android.content.DialogInterface.BUTTON_NEGATIVE).performClick();
        shadowOf(Looper.getMainLooper()).idle();
        assertEquals(false, Store.flag(app, Store.DELETE_AFTER_PASTE));
        assertEquals(false, sw.isChecked());

        a.findViewById(R.id.row_delete).performClick();
        ((android.app.AlertDialog) org.robolectric.shadows.ShadowDialog.getLatestDialog()).getButton(android.content.DialogInterface.BUTTON_POSITIVE).performClick();
        shadowOf(Looper.getMainLooper()).idle();
        android.app.AlertDialog second = (android.app.AlertDialog) org.robolectric.shadows.ShadowDialog.getLatestDialog();
        assertEquals("Are you sure?", shadowOf(second).getTitle().toString());
        assertEquals("not on after only one yes", false, Store.flag(app, Store.DELETE_AFTER_PASTE));
        second.getButton(android.content.DialogInterface.BUTTON_POSITIVE).performClick();
        shadowOf(Looper.getMainLooper()).idle();
        assertEquals(true, Store.flag(app, Store.DELETE_AFTER_PASTE));
        assertEquals(true, sw.isChecked());

        // Off needs no confirmation.
        a.findViewById(R.id.row_delete).performClick();
        assertEquals(false, Store.flag(app, Store.DELETE_AFTER_PASTE));
    }

    @Test
    public void nearLinkSaysHelloAndAnswersOnlyAnAuthenticPull() throws Exception {
        java.net.DatagramSocket pcSock;
        try {
            pcSock = new java.net.DatagramSocket(Proto.UDP_PORT, java.net.InetAddress.getByName("127.0.0.1"));
        } catch (java.net.BindException e) {
            org.junit.Assume.assumeTrue("UDP port in use by a running receiver", false);
            return;
        }
        Store.add(app, LINK.replaceAll("&a=[^&]*", "") + "&a=127.0.0.1");
        Proto.Pc pc = Store.pcs(app).get(0);
        java.util.concurrent.atomic.AtomicInteger pulls = new java.util.concurrent.atomic.AtomicInteger();
        NearLink link = new NearLink(app) {
            @Override
            void onPull(Proto.Pc p) {
                pulls.incrementAndGet();
            }
        };
        try {
            pcSock.setSoTimeout(3000);
            link.start();
            byte[] b = new byte[64];
            java.net.DatagramPacket hello = new java.net.DatagramPacket(b, b.length);
            do { pcSock.receive(hello); } while (hello.getLength() != 44);
            byte[] h = java.util.Arrays.copyOf(b, 44);
            assertEquals("DCH1", new String(h, 0, 4, "US-ASCII"));
            assertArrayEquals(java.util.Arrays.copyOfRange(Proto.hmac(pc.key, "dropcopy/v1/hello".getBytes("US-ASCII"), java.util.Arrays.copyOfRange(h, 12, 28)), 0, 16),
                    java.util.Arrays.copyOfRange(h, 28, 44));

            byte[] nonce = new byte[16];
            new Random(1).nextBytes(nonce);
            java.io.ByteArrayOutputStream forged = new java.io.ByteArrayOutputStream();
            forged.write("DCU1".getBytes("US-ASCII"));
            forged.write(pc.id);
            forged.write(nonce);
            forged.write(new byte[16]); // wrong tag
            pcSock.send(new java.net.DatagramPacket(forged.toByteArray(), 44, hello.getSocketAddress()));
            java.io.ByteArrayOutputStream real = new java.io.ByteArrayOutputStream();
            real.write("DCU1".getBytes("US-ASCII"));
            real.write(pc.id);
            real.write(nonce);
            real.write(Proto.hmac(pc.key, "dropcopy/v1/pull".getBytes("US-ASCII"), nonce), 0, 16);
            for (int k = 0; k < 2; k++) pcSock.send(new java.net.DatagramPacket(real.toByteArray(), 44, hello.getSocketAddress()));
            for (int k = 0; k < 40 && pulls.get() == 0; k++) Thread.sleep(50);
            Thread.sleep(300);
            assertEquals("one preview for a real pull sent twice; none for the forged one", 1, pulls.get());
        } finally {
            link.stop();
            pcSock.close();
        }
    }

    /** Against a real receiver (DROPCOPY_TEST_LINK, DROPCOPY_TEST_HOME): the PC lists the file for deletion. */
    @Test
    public void deleteAfterPasteReachesARealReceiver() throws Exception {
        String link = System.getenv("DROPCOPY_TEST_LINK"), home = System.getenv("DROPCOPY_TEST_HOME");
        assumeFalse("no live receiver configured", link == null || link.isEmpty() || home == null);
        Store.add(app, link);
        Store.setFlag(app, Store.DELETE_AFTER_PASTE, true);
        File f = File.createTempFile("paste-me", ".jpg");
        Files.write(f.toPath(), new byte[]{1, 2, 3});
        Transfer.send(app, Collections.singletonList(Transfer.item(app, Uri.fromFile(f))), null, null);
        Thread.sleep(300);
        String list = new String(Files.readAllBytes(new File(home, "delete-pending.txt").toPath()), "UTF-8");
        assertEquals("one file listed: " + list, 1, list.trim().split("\n").length);
    }

    /**
     * The phone serves the PC app with its pairing appended, over plain HTTP. With
     * DROPCOPY_TEST_EXE (a real program) and DROPCOPY_TEST_OUT set, it writes what a PC downloaded,
     * so the PC side can be run on it.
     */
    @Test
    public void phoneServesThePcAppWithItsPairing() throws Exception {
        String exePath = System.getenv("DROPCOPY_TEST_EXE"), out = System.getenv("DROPCOPY_TEST_OUT");
        byte[] exe = exePath != null ? Files.readAllBytes(new File(exePath).toPath()) : "MZ fake program".getBytes("US-ASCII");
        String link = PcSetup.newLink();
        java.util.concurrent.atomic.AtomicReference<String> who = new java.util.concurrent.atomic.AtomicReference<>();
        PcSetup.Server server;
        try {
            server = new PcSetup.Server(PcSetup.withLink(exe, link), PcSetup.page(app, "Galaxy <S20+>", exe.length), who::set);
        } catch (java.net.BindException e) {
            org.junit.Assume.assumeTrue("port in use", false);
            return;
        }
        try {
            java.net.HttpURLConnection page = (java.net.HttpURLConnection) new java.net.URL("http://127.0.0.1:" + PcSetup.PORT + "/").openConnection();
            String html = new String(readAll(page.getInputStream()), "UTF-8");
            assertTrue(html.contains("href=\"/DropCopy-Setup.exe\""));
            assertTrue("no faded logo on the page (the user's call)", !html.contains("class=\"corner\"") && !html.contains("opacity=\".13\""));
            assertTrue("phone name filled in, escaped", html.contains("Galaxy &lt;S20+&gt;") && !html.contains("{{PHONE}}") && !html.contains("{{SIZE}}"));
            java.net.HttpURLConnection dl = (java.net.HttpURLConnection) new java.net.URL("http://127.0.0.1:" + PcSetup.PORT + "/DropCopy-Setup.exe").openConnection();
            assertTrue(dl.getHeaderField("Content-Disposition").contains("DropCopy-Setup.exe"));
            byte[] got = readAll(dl.getInputStream());
            assertArrayEquals(exe, java.util.Arrays.copyOf(got, exe.length));
            String tail = new String(got, exe.length, got.length - exe.length - 12, "UTF-8");
            assertEquals(link, tail);
            assertEquals("DCSETUP1", new String(got, got.length - 8, 8, "US-ASCII"));
            for (int k = 0; k < 40 && who.get() == null; k++) Thread.sleep(25);
            assertEquals("127.0.0.1", who.get());
            if (out != null) {
                Files.write(new File(out).toPath(), got);
                Files.write(new File(out + ".link").toPath(), link.getBytes("UTF-8"));
            }
        } finally {
            server.stop();
        }
    }

    private static byte[] readAll(java.io.InputStream in) throws java.io.IOException {
        java.io.ByteArrayOutputStream o = new java.io.ByteArrayOutputStream();
        byte[] b = new byte[65536];
        for (int n; (n = in.read(b)) > 0; ) o.write(b, 0, n);
        return o.toByteArray();
    }

    /** Already paired with one PC: the second button scans a new code (here, with no Play
     *  services scanner, it falls back to pasting the link, which proves the scan was started). */
    @Test
    public void pairedPhoneCanScanAnotherPc() {
        Store.add(app, LINK);
        MainActivity a = Robolectric.buildActivity(MainActivity.class).setup().get();
        android.widget.Button b = a.findViewById(R.id.check);
        assertEquals("Scan new PC", b.getText().toString());
        assertEquals(android.view.View.VISIBLE, b.getVisibility());
        b.performClick();
        assertEquals("the scanner was asked for", android.view.View.VISIBLE, a.findViewById(R.id.paste_row).getVisibility());
        // A second PC's link adds it alongside the first, and makes it the one used.
        String second = "dropcopy://pair?v=1&id=8899aabbccddeeff&k=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8&n=Laptop&p=48650";
        ((android.widget.EditText) a.findViewById(R.id.link)).setText(second);
        a.findViewById(R.id.pair).performClick();
        assertEquals(2, Store.pcs(app).size());
        assertEquals("Laptop", Store.pcs(app).get(0).name);
    }

    @Test
    public void unpairedPhoneShowsNoSettingsYet() {
        MainActivity a = Robolectric.buildActivity(MainActivity.class).setup().get();
        assertEquals(android.view.View.GONE, a.findViewById(R.id.needs_card).getVisibility());
    }

    @Test
    public void pairingLinkOpensAppStoresPcAndPublishesShareIcon() {
        Intent open = new Intent(Intent.ACTION_VIEW, Uri.parse(LINK));
        Robolectric.buildActivity(MainActivity.class, open).setup();

        List<Proto.Pc> pcs = Store.pcs(app);
        assertEquals(1, pcs.size());
        assertEquals("My PC", pcs.get(0).name);
        assertEquals("192.168.1.5", pcs.get(0).host);
        assertEquals(32, pcs.get(0).key.length);

        List<ShortcutInfo> shortcuts = app.getSystemService(ShortcutManager.class).getDynamicShortcuts();
        assertEquals(1, shortcuts.size());
        assertEquals("My PC", shortcuts.get(0).getShortLabel().toString());
        assertTrue(shortcuts.get(0).getCategories().contains(Store.CATEGORY));
        assertEquals(Store.AUTO_SHORTCUT, shortcuts.get(0).getId());
    }

    @Test
    public void twoPcsStillMeanOneShareIconThatPicksByItself() {
        Store.add(app, LINK);
        Store.add(app, LINK.replace("0011223344556677", "8899aabbccddeeff").replace("My%20PC", "Desk"));
        List<ShortcutInfo> shortcuts = app.getSystemService(ShortcutManager.class).getDynamicShortcuts();
        assertEquals("one icon in the top row, not one per PC", 1, shortcuts.size());
        assertEquals("My PC", shortcuts.get(0).getShortLabel().toString());
        assertEquals("Send to my PC", shortcuts.get(0).getLongLabel().toString());

        // Its tap carries no PC: SendService asks which one is in use.
        Intent share = new Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, "x")
                .putExtra(Intent.EXTRA_SHORTCUT_ID, Store.AUTO_SHORTCUT);
        Robolectric.buildActivity(ShareActivity.class, share).create();
        Intent svc = shadowOf(app).getNextStartedService();
        assertEquals(SendService.class.getName(), svc.getComponent().getClassName());
        assertNull(svc.getStringExtra("pc"));

        // An icon an older version left behind for one PC also picks by itself now.
        Robolectric.buildActivity(ShareActivity.class, share.putExtra(Intent.EXTRA_SHORTCUT_ID, "pc-0011223344556677")).create();
        assertNull(shadowOf(app).getNextStartedService().getStringExtra("pc"));
    }

    @Test
    public void pcsAreIntroducedWithAPeerKeyNotTheirOwn() {
        Proto.Pc a = Proto.Pc.parse(LINK);
        Proto.Pc b = Proto.Pc.parse(LINK.replace("0011223344556677", "8899aabbccddeeff").replace("My%20PC", "Desk"));
        String link = Proto.peerLink(b, a);
        assertTrue(link.startsWith("dropcopy://peer?v=2&id=8899aabbccddeeff&k="));
        assertFalse("never the other PC's own key", link.contains(java.util.Base64.getUrlEncoder().withoutPadding().encodeToString(b.key)));
        byte[] body = Proto.introductions(java.util.Arrays.asList(a, b)).introFor(a);
        assertEquals(link, new String(body, 2, body.length - 2, java.nio.charset.StandardCharsets.UTF_8));
        // The v2 mark survives being stored.
        a.v2 = true;
        assertTrue(Proto.Pc.parse(a.toLink()).v2);
    }

    @Test
    public void shareSheetHandsFilesToForegroundServiceAndCloses() {
        Store.add(app, LINK);
        Uri photo = Uri.parse("content://media/external/images/media/42");
        Intent share = new Intent(Intent.ACTION_SEND).setType("image/jpeg")
                .putExtra(Intent.EXTRA_STREAM, photo)
                .putExtra(Intent.EXTRA_SHORTCUT_ID, "pc-0011223344556677");
        ActivityController<ShareActivity> c = Robolectric.buildActivity(ShareActivity.class, share).create();
        assertTrue("share target must close at once", c.get().isFinishing());

        Intent svc = shadowOf(app).getNextStartedService();
        assertNotNull(svc);
        assertEquals(SendService.class.getName(), svc.getComponent().getClassName());
        ArrayList<Uri> uris = svc.getParcelableArrayListExtra("uris");
        assertEquals(Collections.singletonList(photo), uris);
        assertEquals("0011223344556677", svc.getStringExtra("pc"));
        assertTrue("read grant must travel to the service", (svc.getFlags() & Intent.FLAG_GRANT_READ_URI_PERMISSION) != 0);
        assertEquals(photo, svc.getClipData().getItemAt(0).getUri());
    }

    @Test
    public void shareSheetForwardsMultipleFilesAndText() {
        Store.add(app, LINK);
        ArrayList<Uri> many = new ArrayList<>();
        many.add(Uri.parse("content://a/1"));
        many.add(Uri.parse("content://a/2"));
        Robolectric.buildActivity(ShareActivity.class,
                new Intent(Intent.ACTION_SEND_MULTIPLE).setType("image/*").putParcelableArrayListExtra(Intent.EXTRA_STREAM, many)).create();
        Intent svc = shadowOf(app).getNextStartedService();
        assertEquals(many, svc.getParcelableArrayListExtra("uris"));
        assertEquals(2, svc.getClipData().getItemCount());

        Robolectric.buildActivity(ShareActivity.class,
                new Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, "https://example.com")).create();
        svc = shadowOf(app).getNextStartedService();
        assertEquals("https://example.com", svc.getStringExtra("text"));
    }

    @Test
    public void shareWithoutPairingOpensSetupInstead() {
        Robolectric.buildActivity(ShareActivity.class,
                new Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, "x")).create();
        assertEquals(null, shadowOf(app).getNextStartedService());
        assertEquals(MainActivity.class.getName(), shadowOf(app).getNextStartedActivity().getComponent().getClassName());
    }

    @Test
    public void clipboardTileActivitySendsClipboardText() {
        Store.add(app, LINK);
        app.getSystemService(ClipboardManager.class).setPrimaryClip(ClipData.newPlainText("", "copied on the phone"));
        ActivityController<ClipActivity> c = Robolectric.buildActivity(ClipActivity.class).create().start().resume();
        c.get().onWindowFocusChanged(true);
        Intent svc = shadowOf(app).getNextStartedService();
        assertNotNull(svc);
        assertEquals("copied on the phone", svc.getStringExtra("text"));
        assertTrue(c.get().isFinishing());
    }

    /** Full path through SendService to a real receiver: set DROPCOPY_TEST_LINK and DROPCOPY_TEST_DIR. */
    @Test
    public void sendServiceDeliversToRealReceiver() throws Exception {
        String link = System.getenv("DROPCOPY_TEST_LINK"), dir = System.getenv("DROPCOPY_TEST_DIR");
        assumeFalse("no live receiver configured", link == null || link.isEmpty());
        Store.add(app, link);

        byte[] data = new byte[2_500_000];
        new Random(7).nextBytes(data);
        Uri uri = Uri.parse("content://dev.dropcopy.test/photo");
        shadowOf(app.getContentResolver()).registerInputStreamSupplier(uri, () -> new ByteArrayInputStream(data)); // fresh per open, like a real file
        File[] before = new File(dir).listFiles();
        int countBefore = before == null ? 0 : before.length;

        ArrayList<Uri> uris = new ArrayList<>(Collections.singletonList(uri));
        Intent start = new Intent(app, SendService.class).putParcelableArrayListExtra("uris", uris);
        Robolectric.buildService(SendService.class, start).create().startCommand(0, 1);

        File got = null;
        for (int i = 0; i < 100 && got == null; i++) {
            Thread.sleep(50);
            shadowOf(Looper.getMainLooper()).idle();
            File[] now = new File(dir).listFiles((d, n) -> n.startsWith("shared_"));
            if (now != null && now.length > 0 && new File(dir).listFiles().length > countBefore) {
                java.util.Arrays.sort(now, (x, y) -> Long.compare(y.lastModified(), x.lastModified()));
                got = now[0]; // the newest: other live tests write here too
            }
        }
        assertNotNull("file did not arrive on the PC; app said: " + org.robolectric.shadows.ShadowToast.getTextOfLatestToast(), got);
        assertArrayEquals(data, Files.readAllBytes(got.toPath()));
        assertTrue("last address remembered", Store.pcs(app).get(0).host != null);
    }

    /** Offline: the send is queued, then delivered by the outbox once the PC is reachable. */
    @Test
    public void unreachablePcQueuesThenDeliversWhenBack() throws Exception {
        String link = System.getenv("DROPCOPY_TEST_LINK"), dir = System.getenv("DROPCOPY_TEST_DIR");
        assumeFalse("no live receiver configured", link == null || link.isEmpty());
        Proto.Pc live = Proto.Pc.parse(link);
        // Same PC, but "away": nothing listens on this port and discovery gets no answer.
        Proto.Pc away = new Proto.Pc(new byte[8], live.key, live.name, "127.0.0.1", 9);
        Store.add(app, away.toLink());

        byte[] data = new byte[300_000];
        new Random(11).nextBytes(data);
        Uri uri = Uri.parse("content://dev.dropcopy.test/queued");
        shadowOf(app.getContentResolver()).registerInputStreamSupplier(uri, () -> new ByteArrayInputStream(data)); // fresh per open, like a real file
        try {
            Transfer.run(app, Collections.singletonList(uri), null, null, null);
            throw new AssertionError("expected the PC to be unreachable");
        } catch (Proto.Unreachable expected) {
            Outbox.add(app, Collections.singletonList(uri), null, null);
        }
        assertTrue(!Outbox.isEmpty(app));
        assertNotNull("retry job scheduled", app.getSystemService(android.app.job.JobScheduler.class).getPendingJob(7));
        try {
            Outbox.flush(app);
            throw new AssertionError("still away");
        } catch (Proto.Unreachable expected) {
            assertTrue("kept for later", !Outbox.isEmpty(app));
        }

        Store.forgetAll(app);
        Store.add(app, link); // the PC is back
        assertEquals(1, Outbox.flush(app));
        assertTrue(Outbox.isEmpty(app));
        File[] got = new File(dir).listFiles((d, n) -> n.startsWith("shared_"));
        boolean found = false;
        for (File f : got) found |= java.util.Arrays.equals(data, Files.readAllBytes(f.toPath()));
        assertTrue("queued file arrived intact", found);
    }

    @Test
    public void proximityTokenMatchesThePcSide() {
        byte[] key = new byte[32];
        java.util.Arrays.fill(key, (byte) 7);
        StringBuilder hex = new StringBuilder();
        for (byte b : Proto.nearToken(key, 29_000_000L)) hex.append(String.format("%02x", b & 0xff));
        assertEquals("c2a8ab18665b2517", hex.toString()); // same vector as pc/src/near.rs
        byte[] p = Proto.nearPayload(key, 29_000_000L);
        assertEquals(10, p.length);
        assertEquals('D', p[0]);
        assertEquals('C', p[1]);
    }

    @Test
    public void updaterComparesVersionsAndReadsReleases() throws Exception {
        assertTrue(Updater.isNewer("v0.3.1", "0.3.0"));
        assertTrue(Updater.isNewer("1.0.0", "0.99.9"));
        assertTrue(!Updater.isNewer("0.3.0", "0.3.0"));
        assertTrue(!Updater.isNewer("0.2.9", "0.3.0"));
        assertTrue(!Updater.isNewer("garbage", "0.3.0"));
        String json = "{\"tag_name\":\"v0.4.0\",\"body\":\"notes\",\"assets\":["
                + "{\"name\":\"dropcopy.exe\",\"browser_download_url\":\"https://x/exe\"},"
                + "{\"name\":\"DropCopy.apk\",\"browser_download_url\":\"https://x/apk\",\"digest\":\"sha256:" + "ab".repeat(32) + "\"}]}";
        Updater.Release r = Updater.parse(json);
        assertEquals("0.4.0", r.version);
        assertEquals("https://x/apk", r.url);
        assertEquals(32, r.sha256.length);
        assertEquals((byte) 0xab, r.sha256[0]);
        assertEquals(null, Updater.parse("{\"tag_name\":\"v1.0.0\",\"assets\":[]}"));
    }

    @Test
    public void mainScreenShowsPairedPcAndSwitchesStartTheService() {
        Store.add(app, LINK);
        org.robolectric.android.controller.ActivityController<MainActivity> c = Robolectric.buildActivity(MainActivity.class).setup();
        MainActivity a = c.get();
        assertEquals("My PC", ((android.widget.TextView) a.findViewById(R.id.pc_name)).getText().toString());
        assertEquals("Check connection", ((android.widget.Button) a.findViewById(R.id.scan)).getText().toString());
        assertEquals(android.view.View.VISIBLE, a.findViewById(R.id.check).getVisibility());
        // Tapping the row (not only the switch) turns the setting on and starts the background service.
        shadowOf(app).grantPermissions(android.Manifest.permission.BLUETOOTH_ADVERTISE);
        a.findViewById(R.id.row_near).performClick();
        assertTrue(Store.flag(app, Store.NEAR));
        Intent svc = shadowOf(app).getNextStartedService();
        assertNotNull(svc);
        assertEquals(WatchService.class.getName(), svc.getComponent().getClassName());
    }

    private static final String PHONE_LINK = "dropcopy://pair?v=1&id=a1a2a3a4a5a6a7a8"
            + "&k=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8&n=Galaxy%20S26&p=48650&a=192.0.2.9&t=phone";

    @Test
    public void scanningAnotherPhoneGivesItItsOwnShareIcon() {
        Store.add(app, LINK);
        Robolectric.buildActivity(MainActivity.class, new Intent(Intent.ACTION_VIEW, Uri.parse(PHONE_LINK))).setup();
        assertEquals("a phone is not a PC", 1, Store.pcs(app).size());
        List<Proto.Pc> phones = Store.phones(app);
        assertEquals(1, phones.size());
        assertEquals("Galaxy S26", phones.get(0).name);
        assertTrue(phones.get(0).phone);

        List<ShortcutInfo> shortcuts = app.getSystemService(ShortcutManager.class).getDynamicShortcuts();
        assertEquals("my PC and the phone", 2, shortcuts.size());
        ShortcutInfo phone = null;
        for (ShortcutInfo si : shortcuts) if (si.getId().startsWith(Store.PHONE_SHORTCUT)) phone = si;
        assertNotNull(phone);
        assertEquals("Galaxy S26", phone.getShortLabel().toString());
        assertTrue(phone.getCategories().contains(Store.CATEGORY));

        // Its icon sends to that phone, not to a PC.
        Intent share = new Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, "hi")
                .putExtra(Intent.EXTRA_SHORTCUT_ID, phone.getId());
        Robolectric.buildActivity(ShareActivity.class, share).create();
        Intent svc = null;
        for (Intent i; (i = shadowOf(app).getNextStartedService()) != null; ) {
            if (SendService.class.getName().equals(i.getComponent().getClassName())) svc = i;
        }
        assertNotNull(svc);
        assertEquals(SendService.PHONE + "a1a2a3a4a5a6a7a8", svc.getStringExtra("pc"));
    }

    @Test
    public void thisPhonesOwnCodeIsStableAndNeverPairsWithItself() {
        Proto.Pc me = Store.me(app);
        assertTrue(me.phone);
        assertEquals(me.hexId(), Store.me(app).hexId());
        assertArrayEquals(me.key, Store.me(app).key);
        Proto.Pc parsed = Proto.Pc.parse(me.toLink());
        assertTrue("the code says it is a phone", parsed.phone);
        Store.rememberPhone(app, parsed);
        assertTrue(Store.phones(app).isEmpty());
    }
}
