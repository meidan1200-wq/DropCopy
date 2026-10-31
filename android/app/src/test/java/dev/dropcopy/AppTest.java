package dev.dropcopy;

import static org.junit.Assert.assertArrayEquals;
import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertNotNull;
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
        assertEquals("pc-0011223344556677", shortcuts.get(0).getId());
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
        shadowOf(app.getContentResolver()).registerInputStream(uri, new ByteArrayInputStream(data));
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
            if (now != null && now.length > 0 && new File(dir).listFiles().length > countBefore) got = now[now.length - 1];
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
        shadowOf(app.getContentResolver()).registerInputStream(uri, new ByteArrayInputStream(data));
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
}
