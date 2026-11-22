package dev.dropcopy;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.ContentResolver;
import android.content.ContentValues;
import android.content.Context;
import android.content.Intent;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.net.Uri;
import android.os.Handler;
import android.os.Looper;
import android.provider.MediaStore;
import java.io.FileNotFoundException;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;

/**
 * PC to phone. The PC keeps what you send it for this phone; the phone collects it when the PC
 * knocks (while the screen is on), when the app opens, or after any send that hears "something
 * is waiting". Pictures land in Pictures/DropCopy, videos in Movies/DropCopy, everything else in
 * Download/DropCopy; text goes on the clipboard. A notification opens what arrived.
 */
final class Receiver {
    private static final String CHANNEL = "receive";
    private static final Object LOCK = new Object();

    private Receiver() {}

    /** Collects from every PC that says something is waiting. */
    static int collect(Context c) {
        List<Proto.Pc> pcs = Store.pcs(c);
        if (pcs.isEmpty()) return 0;
        int n = 0;
        for (Proto.Status st : Proto.ask(pcs, Proto.broadcastAddresses(), Transfer.routes(c), 400)) {
            if (!st.waiting()) continue;
            st.pc.host = st.host;
            st.pc.port = st.port;
            try { n += fetch(c, st.pc); } catch (IOException ignored) { }
        }
        return n;
    }

    /** Collects what {@code pc} has for this phone. One at a time: a knock and a send may overlap. */
    static int fetch(Context c, Proto.Pc pc) throws IOException {
        synchronized (LOCK) {
            Saver saver = new Saver(c);
            List<Proto.Pc> all = Store.pcs(c);
            Proto.Item intro = all.size() > 1 ? Proto.introductions(all) : null;
            int n = Proto.fetch(Collections.singletonList(pc), Proto.broadcastAddresses(), Transfer.routes(c),
                    Transfer.deviceName(c), Store.phoneId(c), intro, saver);
            Store.remember(c, pc);
            if (n > 0) saver.announce(pc.name);
            return n;
        }
    }

    /** Writes into MediaStore as the records arrive; a file appears in the gallery once complete. */
    static final class Saver implements Proto.Sink {
        private final Context c;
        private final ContentResolver cr;
        private Uri current;
        final List<Uri> saved = new ArrayList<>();
        final List<String> mimes = new ArrayList<>();
        final List<String> names = new ArrayList<>();
        String text;

        Saver(Context c) {
            this.c = c.getApplicationContext();
            this.cr = c.getContentResolver();
        }

        static String[] place(String mime) {
            if (mime.startsWith("image/")) return new String[]{"Pictures/DropCopy", MediaStore.Images.Media.EXTERNAL_CONTENT_URI.toString()};
            if (mime.startsWith("video/")) return new String[]{"Movies/DropCopy", MediaStore.Video.Media.EXTERNAL_CONTENT_URI.toString()};
            return new String[]{"Download/DropCopy", MediaStore.Downloads.EXTERNAL_CONTENT_URI.toString()};
        }

        @Override
        public OutputStream begin(String name, String mime, long size) throws IOException {
            if (mime == null || mime.isEmpty()) mime = "application/octet-stream";
            // Only the last part of a name, without characters Android's storage refuses.
            name = name.replace('\\', '/');
            name = name.substring(name.lastIndexOf('/') + 1).replaceAll("[\\x00-\\x1f:*?\"<>|]", "_");
            if (name.isEmpty() || name.startsWith(".")) name = "file" + name;
            String[] where = place(mime);
            ContentValues v = new ContentValues();
            v.put(MediaStore.MediaColumns.DISPLAY_NAME, name);
            v.put(MediaStore.MediaColumns.MIME_TYPE, mime);
            v.put(MediaStore.MediaColumns.RELATIVE_PATH, where[0]);
            v.put(MediaStore.MediaColumns.IS_PENDING, 1);
            current = cr.insert(Uri.parse(where[1]), v);
            if (current == null) throw new IOException("Couldn't save " + name);
            OutputStream os = cr.openOutputStream(current);
            if (os == null) throw new FileNotFoundException(name);
            names.add(name);
            mimes.add(mime);
            return os;
        }

        @Override
        public void end() {
            ContentValues v = new ContentValues();
            v.put(MediaStore.MediaColumns.IS_PENDING, 0);
            cr.update(current, v, null, null);
            saved.add(current);
            current = null;
        }

        @Override
        public void abort() {
            if (current != null) {
                try { cr.delete(current, null, null); } catch (RuntimeException ignored) { }
                current = null;
            }
        }

        @Override
        public void text(String t) {
            text = t;
            // Writing the clipboard is allowed from the background; Android shows its own small "copied" note.
            CountDownLatch done = new CountDownLatch(1);
            new Handler(Looper.getMainLooper()).post(() -> {
                ClipboardManager cm = c.getSystemService(ClipboardManager.class);
                if (cm != null) cm.setPrimaryClip(ClipData.newPlainText("DropCopy", t));
                done.countDown();
            });
            try { done.await(2, TimeUnit.SECONDS); } catch (InterruptedException ignored) { }
        }

        /** One notification for what arrived: tap to open it. */
        void announce(String from) {
            NotificationManager nm = c.getSystemService(NotificationManager.class);
            if (nm == null) return;
            nm.createNotificationChannel(new NotificationChannel(CHANNEL, "Received with DropCopy", NotificationManager.IMPORTANCE_DEFAULT));
            String title;
            String body;
            if (saved.isEmpty()) {
                title = "Copied from " + from;
                body = text == null ? "" : text;
            } else if (saved.size() == 1) {
                title = names.get(0);
                body = "From " + from + (text != null ? " · text copied" : "");
            } else {
                title = saved.size() + " files from " + from;
                body = "In " + place(mimes.get(0))[0] + (text != null ? " · text copied" : "");
            }
            Notification.Builder b = new Notification.Builder(c, CHANNEL)
                    .setSmallIcon(R.drawable.ic_notify)
                    .setContentTitle(title)
                    .setContentText(body)
                    .setAutoCancel(true);
            if (!saved.isEmpty()) {
                Uri first = saved.get(0);
                Intent view = new Intent(Intent.ACTION_VIEW).setDataAndType(first, mimes.get(0))
                        .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_ACTIVITY_NEW_TASK);
                b.setContentIntent(PendingIntent.getActivity(c, first.hashCode(), view, PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT));
                if (mimes.get(0).startsWith("image/")) {
                    Bitmap preview = thumbnail(first);
                    if (preview != null) b.setLargeIcon(preview).setStyle(new Notification.BigPictureStyle().bigPicture(preview).bigLargeIcon((android.graphics.drawable.Icon) null));
                }
            } else if (text != null) {
                b.setStyle(new Notification.BigTextStyle().bigText(text));
            }
            try {
                nm.notify((int) (System.currentTimeMillis() & 0x7fffffff), b.build());
            } catch (SecurityException ignored) {
                // Notifications not allowed: the files are saved all the same.
            }
        }

        private Bitmap thumbnail(Uri u) {
            try (InputStream in = cr.openInputStream(u)) {
                BitmapFactory.Options o = new BitmapFactory.Options();
                o.inJustDecodeBounds = true;
                BitmapFactory.decodeStream(in, null, o);
                int sample = 1;
                while (Math.max(o.outWidth, o.outHeight) / sample > 1024) sample *= 2;
                try (InputStream in2 = cr.openInputStream(u)) {
                    BitmapFactory.Options o2 = new BitmapFactory.Options();
                    o2.inSampleSize = sample;
                    return BitmapFactory.decodeStream(in2, null, o2);
                }
            } catch (IOException | RuntimeException e) {
                return null;
            }
        }
    }
}
