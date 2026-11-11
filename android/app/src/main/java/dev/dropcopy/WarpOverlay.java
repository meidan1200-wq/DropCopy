package dev.dropcopy;

import android.content.Context;
import android.graphics.PixelFormat;
import android.net.Uri;
import android.os.Handler;
import android.os.Looper;
import android.provider.Settings;
import android.view.WindowManager;

/**
 * The arriving half of the warp, drawn over whatever is on screen (the home screen, usually) for
 * two seconds. It takes no touches. Needs "Display over other apps"; without it the picture only
 * comes as a notification.
 */
final class WarpOverlay {
    private WarpOverlay() {}

    static boolean allowed(Context c) {
        return Settings.canDrawOverlays(c);
    }

    /** Plays the arrival of {@code picture} if allowed and the screen is on; says whether it will. */
    static boolean show(Context c, Uri picture) {
        Context app = c.getApplicationContext();
        if (!allowed(app)) return false;
        android.os.PowerManager pm = app.getSystemService(android.os.PowerManager.class);
        if (pm != null && !pm.isInteractive()) return false;
        new Handler(Looper.getMainLooper()).post(() -> {
            WarpView v = WarpView.create(app, picture, false);
            if (v == null) return;
            WindowManager wm = app.getSystemService(WindowManager.class);
            WindowManager.LayoutParams lp = new WindowManager.LayoutParams(
                    WindowManager.LayoutParams.MATCH_PARENT, WindowManager.LayoutParams.MATCH_PARENT,
                    WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY,
                    WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE | WindowManager.LayoutParams.FLAG_NOT_TOUCHABLE
                            | WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN | WindowManager.LayoutParams.FLAG_LAYOUT_NO_LIMITS,
                    PixelFormat.TRANSLUCENT);
            lp.layoutInDisplayCutoutMode = WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES;
            try {
                wm.addView(v, lp);
            } catch (RuntimeException e) {
                return; // permission taken back meanwhile
            }
            v.play(() -> {
                try { wm.removeView(v); } catch (RuntimeException ignored) { }
            });
        });
        return true;
    }
}
