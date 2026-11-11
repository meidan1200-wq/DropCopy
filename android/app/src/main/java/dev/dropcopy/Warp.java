package dev.dropcopy;

/**
 * The warp between two phones, as numbers only (no Android), so the same frames are drawn on the
 * phone and checked on a desktop. The front camera's punch-hole becomes a small black hole: it
 * opens a little, and the picture spirals into it, rows nearest the hole first, stretching and
 * speeding up as it falls in. On the receiving phone the picture spirals out of that phone's
 * camera hole the same way, rests a moment, then fades into the notification.
 *
 * The picture is drawn as a mesh ({@code Canvas.drawBitmapMesh}): each row of the grid travels to
 * the hole on its own schedule and narrows as it goes; the whole of it turns around the hole.
 */
final class Warp {
    static final int COLS = 12, ROWS = 24;
    /** How far behind the top row the bottom row starts (share of the travel). */
    private static final float LAG = 0.6f;
    /** How far the picture turns around the hole on its way in (radians, right at the hole). */
    private static final float SWIRL = 1.8f;
    /** Within about this distance of the hole (in dp) the turning is strong; far away, gentle. */
    private static final float REACH_DP = 70;

    /** Sender: appear, fall into the hole, ripple. */
    static final int SEND_MS = 1150;
    /** Receiver: hole opens, picture spirals out, rests, fades. */
    static final int RECEIVE_MS = 2200;

    private Warp() {}

    /** One frame. Coordinates in pixels; the card's mesh is in {@link #verts}. */
    static final class Frame {
        final float[] verts = new float[(COLS + 1) * (ROWS + 1) * 2];
        /** Of the picture. */
        float alpha;
        /** Of the dark veil behind it (0..1 of its full strength). */
        float veil;
        /** The hole: a black disc over the camera, and the thin ring turning around it. */
        float holeX, holeY, holeRadius, haloAlpha, haloSpin;
        /** The darkness round the hole (a radial fade to clear), while it is open. */
        float shadowRadius, shadowAlpha;
        /** The ripple that spreads from the hole when something goes through. */
        float ringRadius, ringAlpha;
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
     * {@code ms} into the animation. {@code sending}: the picture leaves; else it arrives. The
     * camera hole is at ({@code hx}, {@code hy}) with radius {@code hr}; {@code dp} is the
     * screen density.
     */
    static Frame frame(boolean sending, long ms, float w, float h, float iw, float ih, float dp,
                       float hx, float hy, float hr, Frame f) {
        if (f == null) f = new Frame();
        float[] c = card(w, h, iw, ih);
        f.holeX = hx;
        f.holeY = hy;
        f.haloSpin = ms * 0.006f;
        float open;
        if (sending) {
            float appear = clamp(ms / 160f);
            float fall = clamp((ms - 160) / 700f);
            open = ease(clamp((ms - 60) / 300f)) * (1 - ease(clamp((ms - 850) / 250f)));
            float[] k = scaled(c, 0.94f + 0.06f * ease(appear));
            genie(fall, k[0], k[1], k[2], k[3], hx, hy, SWIRL, REACH_DP * dp, f.verts);
            f.alpha = ease(appear) * (1 - clamp((fall - 0.9f) / 0.1f));
            f.veil = ease(appear) * (1 - clamp((ms - 800) / 300f));
            ripple(f, (ms - 820) / 330f, hr, dp);
            f.done = ms >= SEND_MS;
        } else {
            float pour = clamp((ms - 260) / 700f);
            float leave = clamp((ms - 1800) / 400f);
            open = ease(clamp(ms / 280f)) * (1 - ease(clamp((ms - 900) / 300f)));
            float[] k = scaled(c, 1 - 0.06f * ease(leave));
            float lift = -h * 0.04f * ease(leave);
            k[1] += lift;
            k[3] += lift;
            genie(1 - pour, k[0], k[1], k[2], k[3], hx, hy, SWIRL, REACH_DP * dp, f.verts);
            f.alpha = clamp(pour * 8) * (1 - ease(leave));
            f.veil = clamp(ms / 250f) * (1 - ease(leave));
            ripple(f, (ms - 120) / 420f, hr, dp);
            f.done = ms >= RECEIVE_MS;
        }
        // The camera hole widens to swallow (or let out) the picture, then closes to the camera again.
        f.holeRadius = hr + 9 * dp * open;
        f.haloAlpha = open;
        f.shadowRadius = hr + 70 * dp * open;
        f.shadowAlpha = 0.55f * open;
        return f;
    }

    /** Two thin rings spreading from the hole: the way through. */
    private static void ripple(Frame f, float q, float hr, float dp) {
        if (q <= 0 || q >= 1) {
            f.ringAlpha = 0;
            f.ringRadius = 0;
            return;
        }
        f.ringRadius = hr + (8 + 64 * easeOut(q)) * dp;
        f.ringAlpha = (1 - q) * 0.8f;
    }

    /**
     * The card [l, t, r, b] falling into the hole (px, py) at progress {@code t} (0: whole card,
     * 1: all of it in the hole). Rows nearer the hole go first and speed up as they fall. Like
     * matter round a black hole, each point turns around the hole the more the closer it is: up
     * to {@code swirl} radians at the hole, a fraction of that {@code reach} pixels away.
     */
    static void genie(float t, float l, float top, float r, float b, float px, float py, float swirl, float reach, float[] out) {
        int i = 0;
        float cx0 = (l + r) / 2, half0 = (r - l) / 2;
        for (int row = 0; row <= ROWS; row++) {
            float v = row / (float) ROWS;
            float p = clamp(t * (1 + LAG) - v * LAG);
            float e = fall(p);
            float y0 = top + v * (b - top);
            float y = y0 + (py - y0) * e;
            float cx = cx0 + (px - cx0) * e;
            float half = half0 * (1 - e);
            for (int col = 0; col <= COLS; col++) {
                float u = col / (float) COLS;
                float x = cx + (u * 2 - 1) * half;
                float dx = x - px, dy = y - py;
                double d = Math.sqrt(dx * dx + dy * dy);
                double a = swirl * e * reach / (d + reach), cos = Math.cos(a), sin = Math.sin(a);
                out[i++] = px + (float) (dx * cos - dy * sin);
                out[i++] = py + (float) (dx * sin + dy * cos);
            }
        }
    }

    /** Falling in: slow to let go, then faster and faster. */
    static float fall(float p) {
        return p * p * (1.6f - 0.6f * p);
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
