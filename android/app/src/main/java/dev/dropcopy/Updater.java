package dev.dropcopy;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.job.JobInfo;
import android.app.job.JobScheduler;
import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageInstaller;
import android.os.Build;
import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;

/**
 * Self-update from the project's GitHub releases, like the PC side: check the latest release,
 * download the APK, verify its SHA-256 against the digest GitHub publishes, and hand it to the
 * system installer. From the second update on Android 12+ installs it without asking, because
 * DropCopy is then the app that installed itself.
 */
final class Updater {
    static final String LATEST = "https://api.github.com/repos/meidan1200-wq/DropCopy/releases/latest";
    static final String ASSET = "DropCopy.apk";
    private static final int JOB_ID = 8;
    private static final String CHANNEL = "updates";

    private Updater() {}

    static final class Release {
        final String version;
        final String url;
        final byte[] sha256;
        final String notes;

        Release(String version, String url, byte[] sha256, String notes) {
            this.version = version;
            this.url = url;
            this.sha256 = sha256;
            this.notes = notes;
        }
    }

    /** "v1.2.3" or "1.2.3" to {1,2,3}; null if it isn't that shape. */
    static int[] parseVersion(String s) {
        if (s == null) return null;
        String[] p = s.trim().replaceFirst("^v", "").split("\\.");
        if (p.length != 3) return null;
        int[] v = new int[3];
        try {
            for (int i = 0; i < 3; i++) v[i] = Integer.parseInt(p[i]);
        } catch (NumberFormatException e) {
            return null;
        }
        return v;
    }

    static boolean isNewer(String candidate, String current) {
        int[] a = parseVersion(candidate), b = parseVersion(current);
        if (a == null || b == null) return false;
        for (int i = 0; i < 3; i++) if (a[i] != b[i]) return a[i] > b[i];
        return false;
    }

    static Release parse(String json) throws JSONException {
        JSONObject o = new JSONObject(json);
        String tag = o.getString("tag_name");
        JSONArray assets = o.getJSONArray("assets");
        for (int i = 0; i < assets.length(); i++) {
            JSONObject a = assets.getJSONObject(i);
            if (!ASSET.equals(a.optString("name"))) continue;
            String digest = a.optString("digest", "");
            byte[] sha = digest.startsWith("sha256:") ? Proto.unhex(digest.substring(7)) : null;
            return new Release(tag.replaceFirst("^v", ""), a.getString("browser_download_url"), sha,
                    o.optString("body", ""));
        }
        return null;
    }

    private static HttpURLConnection open(String url) throws IOException {
        HttpURLConnection c = (HttpURLConnection) new URL(url).openConnection();
        c.setConnectTimeout(8000);
        c.setReadTimeout(30000);
        c.setRequestProperty("Accept", "application/vnd.github+json");
        c.setRequestProperty("User-Agent", "DropCopy-Android/" + BuildConfig.VERSION_NAME);
        return c;
    }

    /** The latest release if it is newer than this build, else null. Throws when offline. */
    static Release check() throws IOException {
        HttpURLConnection c = open(LATEST);
        try {
            if (c.getResponseCode() != 200) throw new IOException("GitHub answered " + c.getResponseCode());
            byte[] body = readAll(c.getInputStream());
            Release r = parse(new String(body, java.nio.charset.StandardCharsets.UTF_8));
            return r != null && isNewer(r.version, BuildConfig.VERSION_NAME) ? r : null;
        } catch (JSONException e) {
            throw new IOException("unreadable release info", e);
        } finally {
            c.disconnect();
        }
    }

    /** Downloads into the app's cache and checks the digest. */
    static File download(Context ctx, Release r) throws IOException {
        File out = new File(ctx.getCacheDir(), "update.apk");
        MessageDigest md;
        try {
            md = MessageDigest.getInstance("SHA-256");
        } catch (NoSuchAlgorithmException e) {
            throw new IOException(e);
        }
        HttpURLConnection c = open(r.url);
        c.setInstanceFollowRedirects(true);
        try (InputStream in = c.getInputStream(); OutputStream os = new FileOutputStream(out)) {
            byte[] buf = new byte[64 * 1024];
            for (int n; (n = in.read(buf)) > 0; ) {
                os.write(buf, 0, n);
                md.update(buf, 0, n);
            }
        } finally {
            c.disconnect();
        }
        if (r.sha256 != null && !MessageDigest.isEqual(md.digest(), r.sha256)) {
            out.delete();
            throw new IOException("download was corrupt");
        }
        return out;
    }

    /** Hands the APK to the system installer; the result arrives at UpdateReceiver. */
    static void install(Context ctx, File apk) throws IOException {
        PackageInstaller pi = ctx.getPackageManager().getPackageInstaller();
        PackageInstaller.SessionParams params = new PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL);
        params.setAppPackageName(ctx.getPackageName());
        if (Build.VERSION.SDK_INT >= 31) {
            params.setRequireUserAction(PackageInstaller.SessionParams.USER_ACTION_NOT_REQUIRED);
        }
        int id = pi.createSession(params);
        try (PackageInstaller.Session s = pi.openSession(id)) {
            try (InputStream in = new java.io.FileInputStream(apk); OutputStream out = s.openWrite("base.apk", 0, apk.length())) {
                byte[] buf = new byte[64 * 1024];
                for (int n; (n = in.read(buf)) > 0; ) out.write(buf, 0, n);
                s.fsync(out);
            }
            Intent done = new Intent(ctx, UpdateReceiver.class);
            PendingIntent pending = PendingIntent.getBroadcast(ctx, id, done,
                    PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_MUTABLE);
            s.commit(pending.getIntentSender());
        }
    }

    /** Check, download and install; returns the new version or null if already current. */
    static String run(Context ctx) throws IOException {
        Release r = check();
        if (r == null) return null;
        install(ctx, download(ctx, r));
        return r.version;
    }

    /** Daily check on an unmetered network, while auto-update is on. */
    static void schedule(Context ctx) {
        JobScheduler js = ctx.getSystemService(JobScheduler.class);
        if (js == null) return;
        if (!Store.autoUpdate(ctx)) {
            js.cancel(JOB_ID);
            return;
        }
        if (js.getPendingJob(JOB_ID) != null) return;
        js.schedule(new JobInfo.Builder(JOB_ID, new ComponentName(ctx, UpdateJob.class))
                .setRequiredNetworkType(JobInfo.NETWORK_TYPE_UNMETERED)
                .setPeriodic(24L * 60 * 60 * 1000)
                .setPersisted(true)
                .build());
    }

    /** Android wants a tap to confirm (first update, or older Android): ask with a notification. */
    static void askToConfirm(Context ctx, Intent confirm) {
        NotificationManager nm = ctx.getSystemService(NotificationManager.class);
        nm.createNotificationChannel(new NotificationChannel(CHANNEL, "Updates", NotificationManager.IMPORTANCE_DEFAULT));
        confirm.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
        PendingIntent tap = PendingIntent.getActivity(ctx, 0, confirm, PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);
        Notification n = new Notification.Builder(ctx, CHANNEL)
                .setSmallIcon(R.drawable.ic_notify)
                .setContentTitle("DropCopy update ready")
                .setContentText("Tap to install")
                .setContentIntent(tap)
                .setAutoCancel(true)
                .build();
        nm.notify(3, n);
    }

    private static byte[] readAll(InputStream in) throws IOException {
        java.io.ByteArrayOutputStream o = new java.io.ByteArrayOutputStream();
        byte[] buf = new byte[16 * 1024];
        for (int n; (n = in.read(buf)) > 0; ) o.write(buf, 0, n);
        return o.toByteArray();
    }
}
