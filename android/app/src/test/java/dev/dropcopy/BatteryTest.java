package dev.dropcopy;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertNull;
import static org.junit.Assert.assertTrue;
import static org.robolectric.Shadows.shadowOf;

import android.Manifest;
import android.app.Application;
import android.app.job.JobInfo;
import android.app.job.JobScheduler;
import android.bluetooth.BluetoothManager;
import android.content.Intent;
import android.os.PowerManager;
import android.provider.MediaStore;
import java.net.DatagramPacket;
import java.net.DatagramSocket;
import java.net.InetAddress;
import org.junit.Test;
import org.junit.runner.RunWith;
import org.robolectric.Robolectric;
import org.robolectric.RobolectricTestRunner;
import org.robolectric.RuntimeEnvironment;
import org.robolectric.annotation.Config;
import org.robolectric.shadows.ShadowPowerManager;

/**
 * What DropCopy does in the background, held to a budget: nothing runs unless the user turned a
 * feature on; Bluetooth and the network link only while the screen is on; no wake locks; jobs
 * wait for a network (updates: for Wi-Fi), and the link sleeps instead of polling.
 */
@RunWith(RobolectricTestRunner.class)
@Config(sdk = 35)
public class BatteryTest {
    private static final String LINK = "dropcopy://pair?v=1&id=0011223344556677"
            + "&k=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8&n=My%20PC&p=48650&a=127.0.0.1";
    private final Application app = RuntimeEnvironment.getApplication();

    @Test
    public void nothingRunsInTheBackgroundUntilAFeatureIsOn() {
        Store.add(app, LINK);
        // Receiving from PCs is on by default; with it off, nothing else is.
        Store.setFlag(app, Store.RECEIVE, false);
        Robolectric.buildActivity(MainActivity.class).setup().pause().stop();
        WatchService.sync(app);
        Intent started;
        while ((started = shadowOf(app).getNextStartedService()) != null) {
            assertFalse("no background service: " + started, WatchService.class.getName().equals(started.getComponent().getClassName()));
        }
        assertNull("no wake lock", ShadowPowerManager.getLatestWakeLock());
        assertTrue("no media observer", shadowOf(app.getContentResolver()).getContentObservers(MediaStore.Images.Media.EXTERNAL_CONTENT_URI).isEmpty());
    }

    @Test
    public void receivingListensOnlyWhileTheScreenIsOnAndNeverUsesBluetooth() {
        Store.add(app, LINK);
        PowerManager pm = app.getSystemService(PowerManager.class);
        shadowOf(pm).setIsInteractive(true);
        WatchService s = Robolectric.buildService(WatchService.class).create().startCommand(0, 1).get();
        assertTrue("listening with the screen on", s.linkNow());
        assertFalse("no Bluetooth for receiving", s.advertisingNow());

        shadowOf(pm).setIsInteractive(false);
        app.sendBroadcast(new Intent(Intent.ACTION_SCREEN_OFF));
        shadowOf(android.os.Looper.getMainLooper()).idle();
        assertFalse("asleep with the screen off", s.linkNow());

        shadowOf(pm).setIsInteractive(true);
        app.sendBroadcast(new Intent(Intent.ACTION_SCREEN_ON));
        shadowOf(android.os.Looper.getMainLooper()).idle();
        assertTrue("back when the screen comes on", s.linkNow());
        s.onDestroy();
        assertFalse(s.linkNow());
        assertNull("no wake lock", ShadowPowerManager.getLatestWakeLock());
    }

    @Test
    public void bluetoothAndTheLinkRunOnlyWhileTheScreenIsOn() {
        Store.add(app, LINK);
        Store.setFlag(app, Store.NEAR, true);
        shadowOf(app).grantPermissions(Manifest.permission.BLUETOOTH_ADVERTISE);
        BluetoothManager bm = app.getSystemService(BluetoothManager.class);
        shadowOf(bm.getAdapter()).setEnabled(true);
        PowerManager pm = app.getSystemService(PowerManager.class);
        shadowOf(pm).setIsInteractive(true);

        WatchService s = Robolectric.buildService(WatchService.class).create().startCommand(0, 1).get();
        assertTrue("advertising with the screen on", s.advertisingNow());

        shadowOf(pm).setIsInteractive(false);
        app.sendBroadcast(new Intent(Intent.ACTION_SCREEN_OFF));
        shadowOf(android.os.Looper.getMainLooper()).idle();
        assertFalse("silent with the screen off", s.advertisingNow());

        shadowOf(pm).setIsInteractive(true);
        app.sendBroadcast(new Intent(Intent.ACTION_SCREEN_ON));
        shadowOf(android.os.Looper.getMainLooper()).idle();
        assertTrue("back when the screen comes on", s.advertisingNow());

        s.onDestroy();
        assertFalse("all stopped with the service", s.advertisingNow());
        assertNull("no wake lock", ShadowPowerManager.getLatestWakeLock());
        assertTrue("near alone watches no photos", shadowOf(app.getContentResolver()).getContentObservers(MediaStore.Images.Media.EXTERNAL_CONTENT_URI).isEmpty());
    }

    @Test
    public void backgroundJobsWaitForTheRightNetwork() {
        Updater.schedule(app);
        RetryJob.schedule(app);
        JobScheduler js = app.getSystemService(JobScheduler.class);
        JobInfo update = null, retry = null;
        for (JobInfo j : js.getAllPendingJobs()) {
            if (j.getService().getClassName().equals(UpdateJob.class.getName())) update = j;
            if (j.getService().getClassName().equals(RetryJob.class.getName())) retry = j;
        }
        assertNotNull(update);
        assertEquals("update check only on Wi-Fi", JobInfo.NETWORK_TYPE_UNMETERED, update.getNetworkType());
        assertEquals("once a day", 24L * 60 * 60 * 1000, update.getIntervalMillis());
        assertNotNull(retry);
        assertEquals("retries wait for a network instead of polling", JobInfo.NETWORK_TYPE_ANY, retry.getNetworkType());
        assertFalse(retry.isPeriodic());
    }

    /** The link wakes for a pull or its next hello (every 15 s), never on a short timer. */
    @Test
    public void theLinkSaysHelloOnlyEvery15Seconds() throws Exception {
        DatagramSocket pc;
        try {
            pc = new DatagramSocket(Proto.UDP_PORT, InetAddress.getByName("127.0.0.1"));
        } catch (java.net.BindException e) {
            org.junit.Assume.assumeTrue("UDP port in use by a running receiver", false);
            return;
        }
        Store.add(app, LINK);
        NearLink link = new NearLink(app);
        int hellos = 0;
        try {
            pc.setSoTimeout(200);
            link.start();
            long end = System.currentTimeMillis() + 4000;
            byte[] b = new byte[64];
            while (System.currentTimeMillis() < end) {
                try {
                    DatagramPacket p = new DatagramPacket(b, b.length);
                    pc.receive(p);
                    if (p.getLength() == 44 && b[2] == 'H') hellos++;
                } catch (java.net.SocketTimeoutException ignored) { }
            }
        } finally {
            link.stop();
            pc.close();
        }
        assertEquals("one hello in 4 s", 1, hellos);
    }
}
