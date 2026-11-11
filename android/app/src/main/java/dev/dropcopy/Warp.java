package dev.dropcopy;

/**
 * The warp between two phones, as numbers only (no Android), so the same frames are drawn on the
 * phone and checked on a desktop. A picture sent to a phone is pulled into the top edge of the
 * sender's screen like a cloth drawn through a ring; on the receiver it pours out of the top edge
 * the same way, rests a moment, then fades into the notification.
 *
 * The picture is drawn as a mesh ({@code Canvas.drawBitmapMesh}): each row of the grid travels to
 * the point on its own schedule, rows nearest the point first, and narrows as it goes.
 */
final class Warp {
    static final int COLS = 12, ROWS = 20;
    /** How far behind the top row the bottom row starts (share of the travel). */
    private static final float LAG = 0.6f;

    /** Sender: appear, pull in, ring. */
    static final int SEND_MS = 950;
    /** Receiver: ring, pour out, rest, fade. */
    static final int RECEIVE_MS = 2100;

    private Warp() {}

    /** One frame. Coordinates in pixels; the card's mesh is in {@link #verts}. */
    static final class Frame {
        final float[] verts = new float[(COLS + 1) * (ROWS + 1) * 2];
        /** Of the picture. */
        float alpha;
        /** Of the dark veil behind it (0..1 of its full strength). */
        float veil;
        /** The ring where the picture goes in or comes out. */
        float ringX, ringY, ringRadius, ringAlpha;
        boolean done;
    }

    /**
     * The card for a {@code iw} x {@code ih} picture on a {@code w} x {@code h} screen: as wide as
     * 72% of the screen, at most 55% of its height, a little above the middle.
     */
    static float[] card(float w, float h, float iw, float ih) {
        float cw = w * 0.72f, ch = cw * ih / Math.max(1, iw);
        if (ch > h * 0.55f) {
            ch = h * 0.55f;
            cw = ch * iw / Math.max(1, ih);
        }
        float cx = w / 2, cy = h * 0.44f;
        return new float[]{cx - cw / 2, cy - ch / 2, cx + cw / 2, cy + ch / 2};
    }

    /**
     * {@code ms} into the animation. {@code sending}: the picture leaves; else it arrives.
     * {@code dp} is the screen density, for the ring's size.
     */
    static Frame frame(boolean sending, long ms, float w, float h, float iw, float ih, float dp, Frame f) {
        if (f == null) f = new Frame();
        float[] c = card(w, h, iw, ih);
        float tx = w / 2, ty = -8 * dp; // just above the screen's top edge
        f.ringX = tx;
        f.ringY = 0;
        if (sending) {
            float appear = clamp(ms / 160f);
            float pull = clamp((ms - 140) / 600f);
            float scale = 0.94f + 0.06f * ease(appear);
            float[] k = scaled(c, scale);
            genie(pull, k[0], k[1], k[2], k[3], tx, ty, 3 * dp, f.verts);
            f.alpha = ease(appear) * (1 - clamp((pull - 0.85f) / 0.15f));
            f.veil = ease(appear) * (1 - clamp((ms - 650) / 300f));
            ring(f, (ms - 600) / 350f, dp);
            f.done = ms >= SEND_MS;
        } else {
            float pour = clamp((ms - 120) / 680f);
            float leave = clamp((ms - 1700) / 400f);
            float scale = 1 - 0.06f * ease(leave);
            float[] k = scaled(c, scale);
            float lift = -h * 0.04f * ease(leave);
            k[1] += lift;
            k[3] += lift;
            genie(1 - pour, k[0], k[1], k[2], k[3], tx, ty, 3 * dp, f.verts);
            f.alpha = clamp(pour * 6) * (1 - ease(leave));
            f.veil = clamp(ms / 250f) * (1 - ease(leave));
            ring(f, ms / 420f, dp);
            f.done = ms >= RECEIVE_MS;
        }
        return f;
    }

    /** Two thin rings opening from the point: the way through. */
    private static void ring(Frame f, float q, float dp) {
        if (q <= 0 || q >= 1) {
            f.ringAlpha = 0;
            f.ringRadius = 0;
            return;
        }
        f.ringRadius = (12 + 64 * easeOut(q)) * dp;
        f.ringAlpha = (1 - q) * 0.9f;
    }

    /**
     * The card [l, t, r, b] pulled into the point (px, py) at progress {@code t} (0: whole card,
     * 1: all of it in the point, {@code tip} pixels wide).
     */
    static void genie(float t, float l, float top, float r, float b, float px, float py, float tip, float[] out) {
        int i = 0;
        float cx0 = (l + r) / 2, half0 = (r - l) / 2;
        for (int row = 0; row <= ROWS; row++) {
            float v = row / (float) ROWS;
            float e = ease(clamp(t * (1 + LAG) - v * LAG));
            float y0 = top + v * (b - top);
            float y = y0 + (py - y0) * e;
            float cx = cx0 + (px - cx0) * e;
            float half = half0 + (tip / 2 - half0) * e;
            for (int col = 0; col <= COLS; col++) {
                float u = col / (float) COLS;
                out[i++] = cx + (u * 2 - 1) * half;
                out[i++] = y;
            }
        }
    }

    private static float[] scaled(float[] c, float s) {
        float cx = (c[0] + c[2]) / 2, cy = (c[1] + c[3]) / 2, hw = (c[2] - c[0]) / 2 * s, hh = (c[3] - c[1]) / 2 * s;
        return new float[]{cx - hw, cy - hh, cx + hw, cy + hh};
    }

    static float clamp(float x) {
        return x < 0 ? 0 : x > 1 ? 1 : x;
    }

    /** Slow, fast, slow. */
    static float ease(float x) {
        return x < 0.5f ? 4 * x * x * x : 1 - (float) Math.pow(-2 * x + 2, 3) / 2;
    }

    static float easeOut(float x) {
        return 1 - (1 - x) * (1 - x) * (1 - x);
    }
}
