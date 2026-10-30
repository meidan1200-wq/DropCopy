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
}
