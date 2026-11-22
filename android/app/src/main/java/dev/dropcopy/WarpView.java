package dev.dropcopy;

import android.animation.ValueAnimator;
import android.content.ContentResolver;
import android.content.Context;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.graphics.BitmapShader;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.Paint;
import android.graphics.RectF;
import android.graphics.Shader;
import android.net.Uri;
import android.view.View;
import android.view.animation.LinearInterpolator;
import java.io.IOException;
import java.io.InputStream;

/**
 * Draws {@link Warp}'s frames: the picture falling into this phone's front-camera hole, or
 * coming out of it.
 */
final class WarpView extends View {
    private static final int BLUE = 0xFF237EF0;
    private final boolean sending;
    private final Bitmap card;
    private final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG | Paint.FILTER_BITMAP_FLAG);
    private final Paint ring = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final Paint hole = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final Paint shadow = new Paint(Paint.ANTI_ALIAS_FLAG);
    /** The front camera's hole, in this view's pixels; found on the first frame. */
    private float hx = Float.NaN, hy, hr;
    private final Warp.Frame frame = new Warp.Frame();
    private final float dp;
    private long ms;
    private Runnable onDone;

    private WarpView(Context c, Bitmap picture, boolean sending) {
        super(c);
        this.sending = sending;
        this.dp = c.getResources().getDisplayMetrics().density;
        this.card = rounded(picture, 18 * dp * picture.getWidth() / (c.getResources().getDisplayMetrics().widthPixels * 0.72f));
        ring.setStyle(Paint.Style.STROKE);
        ring.setStrokeWidth(1.5f * dp);
        ring.setStrokeCap(Paint.Cap.ROUND);
        hole.setColor(Color.BLACK);
        setLayerType(LAYER_TYPE_HARDWARE, null);
    }

    /** Null when the picture can't be read. */
    static WarpView create(Context c, Uri picture, boolean sending) {
        Bitmap b = decode(c.getContentResolver(), picture, 900);
        return b == null ? null : new WarpView(c, b, sending);
    }

    /** Plays once, then calls {@code done} on the main thread. */
    void play(Runnable done) {
        onDone = done;
        int total = sending ? Warp.SEND_MS : Warp.RECEIVE_MS;
        ValueAnimator a = ValueAnimator.ofInt(0, total);
        a.setDuration(total);
        a.setInterpolator(new LinearInterpolator()); // the easing is in Warp
        a.addUpdateListener(v -> {
            ms = (int) v.getAnimatedValue();
            invalidate();
        });
        a.addListener(new android.animation.AnimatorListenerAdapter() {
            @Override
            public void onAnimationEnd(android.animation.Animator animation) {
                if (onDone != null) onDone.run();
                onDone = null;
            }
        });
        a.start();
    }

    @Override
    protected void onDraw(Canvas canvas) {
        int w = getWidth(), h = getHeight();
        if (w == 0 || h == 0) return;
        if (Float.isNaN(hx)) findCamera(w);
        Warp.frame(sending, ms, w, h, card.getWidth(), card.getHeight(), dp, hx, hy, hr, frame);
        if (frame.veil > 0) canvas.drawColor(Color.argb(Math.round(255 * 0.22f * frame.veil), 0, 0, 0));
        if (frame.shadowAlpha > 0.01f) {
            shadow.setShader(new android.graphics.RadialGradient(frame.holeX, frame.holeY, frame.shadowRadius,
                    Color.argb(Math.round(255 * frame.shadowAlpha), 0, 0, 0), Color.TRANSPARENT, Shader.TileMode.CLAMP));
            canvas.drawCircle(frame.holeX, frame.holeY, frame.shadowRadius, shadow);
        }
        if (frame.alpha > 0.01f) {
            paint.setAlpha(Math.round(255 * frame.alpha));
            canvas.drawBitmapMesh(card, Warp.COLS, Warp.ROWS, frame.verts, 0, null, 0, paint);
        }
        if (frame.ringAlpha > 0) {
            for (float k : new float[]{1f, 0.62f}) {
                ring.setColor(BLUE);
                ring.setAlpha(Math.round(255 * frame.ringAlpha * k));
                canvas.drawCircle(frame.holeX, frame.holeY, frame.holeRadius + (frame.ringRadius - frame.holeRadius) * k, ring);
            }
        }
        if (frame.haloAlpha > 0.01f) {
            // A thin ring turning round the hole, part bright, part faint: the matter about to fall in.
            float r = frame.holeRadius * 1.6f + 2 * dp;
            ring.setColor(BLUE);
            ring.setAlpha(Math.round(255 * 0.3f * frame.haloAlpha));
            canvas.drawCircle(frame.holeX, frame.holeY, r, ring);
            ring.setAlpha(Math.round(255 * 0.9f * frame.haloAlpha));
            canvas.drawArc(frame.holeX - r, frame.holeY - r, frame.holeX + r, frame.holeY + r,
                    (float) Math.toDegrees(frame.haloSpin), 200, false, ring);
        }
        canvas.drawCircle(frame.holeX, frame.holeY, frame.holeRadius, hole);
    }

    /**
     * Where the front camera's punch-hole is (the display cutout), so the picture falls into it.
     * Without one, a small hole just above the middle of the top edge.
     */
    private void findCamera(int w) {
        hx = w / 2f;
        hy = -2 * dp;
        hr = 6 * dp;
        android.view.WindowInsets in = getRootWindowInsets();
        android.view.DisplayCutout dc = in == null ? null : in.getDisplayCutout();
        if (dc == null) return;
        android.graphics.Rect best = null;
        for (android.graphics.Rect r : dc.getBoundingRects()) {
            if (r.isEmpty()) continue;
            if (best == null || r.top < best.top) best = r;
        }
        if (best == null) return;
        int[] at = new int[2];
        getLocationInWindow(at);
        float radius = Math.min(best.width(), best.height()) / 2f;
        hx = best.exactCenterX() - at[0];
        // Some phones report the hole from the screen's edge down: its centre sits a radius above the bottom.
        hy = best.bottom - radius - at[1];
        hr = radius;
    }

    /** The picture with rounded corners, ready for the mesh. */
    private static Bitmap rounded(Bitmap src, float radius) {
        Bitmap out = Bitmap.createBitmap(src.getWidth(), src.getHeight(), Bitmap.Config.ARGB_8888);
        Canvas c = new Canvas(out);
        Paint p = new Paint(Paint.ANTI_ALIAS_FLAG);
        p.setShader(new BitmapShader(src, Shader.TileMode.CLAMP, Shader.TileMode.CLAMP));
        c.drawRoundRect(new RectF(0, 0, src.getWidth(), src.getHeight()), radius, radius, p);
        return out;
    }

    /** Decodes at most {@code max} pixels on the long side (with the photo's rotation). */
    static Bitmap decode(ContentResolver cr, Uri u, int max) {
        try {
            BitmapFactory.Options o = new BitmapFactory.Options();
            o.inJustDecodeBounds = true;
            try (InputStream in = cr.openInputStream(u)) {
                BitmapFactory.decodeStream(in, null, o);
            }
            if (o.outWidth <= 0) return null;
            int sample = 1;
            while (Math.max(o.outWidth, o.outHeight) / (sample * 2) >= max) sample *= 2;
            BitmapFactory.Options o2 = new BitmapFactory.Options();
            o2.inSampleSize = sample;
            Bitmap b;
            try (InputStream in = cr.openInputStream(u)) {
                b = BitmapFactory.decodeStream(in, null, o2);
            }
            if (b == null) return null;
            int degrees = 0;
            try (InputStream in = cr.openInputStream(u)) {
                if (in != null) {
                    int or = new android.media.ExifInterface(in).getAttributeInt(android.media.ExifInterface.TAG_ORIENTATION, 1);
                    degrees = or == 6 ? 90 : or == 3 ? 180 : or == 8 ? 270 : 0;
                }
            } catch (IOException | RuntimeException ignored) { }
            if (degrees != 0) {
                android.graphics.Matrix m = new android.graphics.Matrix();
                m.postRotate(degrees);
                b = Bitmap.createBitmap(b, 0, 0, b.getWidth(), b.getHeight(), m, true);
            }
            return b;
        } catch (IOException | RuntimeException | OutOfMemoryError e) {
            return null;
        }
    }
}
