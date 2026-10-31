package dev.dropcopy;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertTrue;
import static org.robolectric.Shadows.shadowOf;

import android.Manifest;
import android.app.Application;
import android.content.ComponentName;
import android.content.Intent;
import android.net.Uri;
import android.os.Build;
import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.io.File;
import java.nio.file.Files;
import java.util.Arrays;
import java.util.List;
import org.junit.Test;
import org.junit.runner.RunWith;
import org.robolectric.Robolectric;
import org.robolectric.RobolectricTestRunner;
import org.robolectric.RuntimeEnvironment;
import org.robolectric.annotation.Config;
import org.robolectric.shadows.ShadowBuild;

/**
 * The same flows on every Android version the app supports (minSdk 29 = Android 10 up to the
 * target, Android 15), and on the makers whose battery managers differ. Each test runs once per
 * SDK listed, so an API used on a version that lacks it fails here, not on someone's phone.
 */
@RunWith(RobolectricTestRunner.class)
@Config(sdk = {29, 30, 31, 33, 34, 35})
public class CompatTest {
    private static final String LINK = "dropcopy://pair?v=1&id=0011223344556677"
            + "&k=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8&n=My%20PC&p=48650&a=192.168.1.5";
    private final Application app = RuntimeEnvironment.getApplication();

    @Test
    public void mainScreenAsksOnlyForPermissionsThisVersionHas() {
        Store.add(app, LINK);
        MainActivity a = Robolectric.buildActivity(MainActivity.class).setup().get();
        a.findViewById(R.id.row_shots).performClick();
        a.findViewById(R.id.row_near).performClick();
        List<String> asked = new java.util.ArrayList<>();
        for (int i = 0; i < 5; i++) {
            org.robolectric.shadows.ShadowActivity.PermissionsRequest r = shadowOf(a).getLastRequestedPermission();
            if (r != null) asked.addAll(Arrays.asList(r.requestedPermissions));
        }
        int sdk = Build.VERSION.SDK_INT;
        assertEquals("photos permission", sdk >= 33, asked.contains(Manifest.permission.READ_MEDIA_IMAGES));
        assertEquals("old photos permission", sdk < 33, asked.contains(Manifest.permission.READ_EXTERNAL_STORAGE));
        assertEquals("notifications permission exists from 13", sdk >= 33, asked.contains(Manifest.permission.POST_NOTIFICATIONS));
        assertEquals("Bluetooth advertise permission exists from 12", sdk >= 31, asked.contains(Manifest.permission.BLUETOOTH_ADVERTISE));
    }

    @Test
    public void shareSheetHandsOverOnEveryVersion() {
        Store.add(app, LINK);
        Uri photo = Uri.parse("content://media/external/images/media/42");
        Intent share = new Intent(Intent.ACTION_SEND).setType("image/jpeg").putExtra(Intent.EXTRA_STREAM, photo);
        assertTrue(Robolectric.buildActivity(ShareActivity.class, share).create().get().isFinishing());
        Intent svc = shadowOf(app).getNextStartedService();
        assertNotNull(svc);
        assertEquals(SendService.class.getName(), svc.getComponent().getClassName());
    }

    @Test
    public void backgroundServiceRunsInTheForegroundWithItsNotification() {
        Store.add(app, LINK);
        Store.setFlag(app, Store.WATCH_SCREENSHOTS, true);
        shadowOf(app).grantPermissions(WatchService.mediaPermission());
        WatchService s = Robolectric.buildService(WatchService.class).create().startCommand(0, 1).get();
        assertNotNull("startForeground was called", shadowOf(s).getLastForegroundNotification());
        s.onDestroy();
    }

    @Test
    public void proximitySetupWindowOpensOnEveryVersion() {
        Store.add(app, LINK);
        MainActivity a = Robolectric.buildActivity(MainActivity.class).setup().get();
        a.findViewById(R.id.row_near).performClick();
        assertNotNull(org.robolectric.shadows.ShadowDialog.getLatestDialog());
    }

    @Test
    public void updateInstallSessionStartsOnEveryVersion() throws Exception {
        File apk = File.createTempFile("update", ".apk");
        Files.write(apk.toPath(), new byte[]{0x50, 0x4b, 3, 4});
        Updater.install(app, apk); // setRequireUserAction is only called where it exists (12+)
        assertFalse(app.getPackageManager().getPackageInstaller().getMySessions().isEmpty());
    }

    @Test
    public void encryptionWorksWithThisVersionsCrypto() throws Exception {
        byte[] key = new byte[32];
        for (int i = 0; i < 32; i++) key[i] = (byte) i;
        Proto.Records tx = new Proto.Records(key), rx = new Proto.Records(key);
        ByteArrayOutputStream wire = new ByteArrayOutputStream();
        byte[] body = "שלום, PC".getBytes(java.nio.charset.StandardCharsets.UTF_8);
        tx.write(wire, Proto.T_TEXT, body);
        byte[] got = rx.read(new java.io.DataInputStream(new ByteArrayInputStream(wire.toByteArray())));
        assertEquals(Proto.T_TEXT, got[0]);
        assertTrue(Arrays.equals(body, Arrays.copyOfRange(got, 1, got.length)));
    }

    /** Xiaomi, Huawei, Oppo…: the auto-start screen is offered; Samsung and Pixel have none. */
    @Test
    public void autoStartScreenIsFoundOnlyWhereTheMakerHasOne() {
        String[][] cases = {
                {"Xiaomi", "com.miui.securitycenter", "com.miui.permcenter.autostart.AutoStartManagementActivity"},
                {"HUAWEI", "com.huawei.systemmanager", "com.huawei.systemmanager.startupmgr.ui.StartupNormalAppListActivity"},
                {"OPPO", "com.coloros.safecenter", "com.coloros.safecenter.permission.startup.StartupAppListActivity"},
                {"vivo", "com.vivo.permissionmanager", "com.vivo.permissionmanager.activity.BgStartUpManagerActivity"},
        };
        for (String[] c : cases) {
            ShadowBuild.setManufacturer(c[0]);
            assertEquals(c[0] + " without the screen installed", null, Oem.autostart(app));
            shadowOf(app.getPackageManager()).addActivityIfNotPresent(new ComponentName(c[1], c[2]));
            Intent i = Oem.autostart(app);
            assertNotNull(c[0], i);
            assertEquals(c[2], i.getComponent().getClassName());
        }
        ShadowBuild.setManufacturer("samsung");
        assertEquals(null, Oem.autostart(app));
        ShadowBuild.setManufacturer("Google");
        assertEquals(null, Oem.autostart(app));
    }

    @Test
    public void onAXiaomiTheNeedsCardOffersAutoStart() {
        ShadowBuild.setManufacturer("Xiaomi");
        shadowOf(app.getPackageManager()).addActivityIfNotPresent(
                new ComponentName("com.miui.securitycenter", "com.miui.permcenter.autostart.AutoStartManagementActivity"));
        Store.add(app, LINK);
        Store.setFlag(app, Store.WATCH_SCREENSHOTS, true);
        MainActivity a = Robolectric.buildActivity(MainActivity.class).setup().get();
        assertEquals(android.view.View.VISIBLE, a.findViewById(R.id.row_ok_oem).getVisibility());
        String text = ((android.widget.TextView) a.findViewById(R.id.oem_detail)).getText().toString();
        assertTrue(text, text.startsWith("Xiaomi stops apps"));
        a.findViewById(R.id.oem_allow).performClick();
        Intent opened = shadowOf(app).getNextStartedActivity();
        assertEquals("com.miui.permcenter.autostart.AutoStartManagementActivity", opened.getComponent().getClassName());
        assertEquals("shown once: the switch can't be read back", android.view.View.GONE, a.findViewById(R.id.row_ok_oem).getVisibility());
    }

    /** Refused, or only "selected photos" (Android 14+): the card says so until all are allowed. */
    @Test
    public void missingPhotoAccessIsShownUntilAllowed() {
        Store.add(app, LINK);
        Store.setFlag(app, Store.WATCH_SCREENSHOTS, true);
        if (Build.VERSION.SDK_INT >= 34) shadowOf(app).grantPermissions("android.permission.READ_MEDIA_VISUAL_USER_SELECTED");
        MainActivity a = Robolectric.buildActivity(MainActivity.class).setup().get();
        assertEquals(android.view.View.VISIBLE, a.findViewById(R.id.row_ok_photos).getVisibility());
        shadowOf(app).grantPermissions(WatchService.mediaPermission());
        a = Robolectric.buildActivity(MainActivity.class).setup().get();
        assertEquals(android.view.View.GONE, a.findViewById(R.id.row_ok_photos).getVisibility());
    }
}
