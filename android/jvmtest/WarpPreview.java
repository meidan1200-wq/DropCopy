package dev.dropcopy;

import java.awt.AlphaComposite;
import java.awt.BasicStroke;
import java.awt.Color;
import java.awt.GradientPaint;
import java.awt.Graphics2D;
import java.awt.RenderingHints;
import java.awt.geom.AffineTransform;
import java.awt.geom.Ellipse2D;
import java.awt.geom.Path2D;
import java.awt.geom.RoundRectangle2D;
import java.awt.image.BufferedImage;
import java.io.File;
import javax.imageio.ImageIO;

/**
 * Draws the warp's frames on a desktop, from the same numbers the phone uses ({@link Warp}), over
 * a stand-in home screen: a look at the animation without a phone.
 *
 *   javac -d out ../app/src/main/java/dev/dropcopy/Warp.java WarpPreview.java
 *   java -cp out dev.dropcopy.WarpPreview <out dir>   (then frames → GIF)
 */
public class WarpPreview {
    static final int W = 360, H = 780;

    public static void main(String[] args) throws Exception {
        File dir = new File(args[0]);
        dir.mkdirs();
        BufferedImage photo = photo(640, 480);
        BufferedImage home = home();
        int n = 0;
        for (boolean sending : new boolean[]{true, false}) {
            int total = sending ? Warp.SEND_MS : Warp.RECEIVE_MS;
            for (int ms = 0; ms <= total + 200; ms += 33) {
                BufferedImage f = render(sending, ms, home, photo);
                ImageIO.write(f, "png", new File(dir, String.format("%s-%04d.png", sending ? "a-send" : "b-receive", n++)));
            }
        }
        System.out.println(n + " frames");
    }

    static BufferedImage render(boolean sending, int ms, BufferedImage home, BufferedImage photo) {
        BufferedImage out = new BufferedImage(W, H, BufferedImage.TYPE_INT_RGB);
        Graphics2D g = out.createGraphics();
        g.setRenderingHint(RenderingHints.KEY_ANTIALIASING, RenderingHints.VALUE_ANTIALIAS_ON);
        g.setRenderingHint(RenderingHints.KEY_INTERPOLATION, RenderingHints.VALUE_INTERPOLATION_BILINEAR);
        g.drawImage(home, 0, 0, null);
        Warp.Frame f = Warp.frame(sending, ms, W, H, photo.getWidth(), photo.getHeight(), 1, HOLE_X, HOLE_Y, HOLE_R, null);
        if (f.veil > 0) {
            g.setColor(new Color(0, 0, 0, Math.round(255 * 0.22f * f.veil)));
            g.fillRect(0, 0, W, H);
        }
        if (f.shadowAlpha > 0.01f) {
            g.setPaint(new java.awt.RadialGradientPaint(f.holeX, f.holeY, f.shadowRadius, new float[]{0f, 1f},
                    new Color[]{new Color(0, 0, 0, f.shadowAlpha), new Color(0, 0, 0, 0)}));
            g.fill(new Ellipse2D.Float(f.holeX - f.shadowRadius, f.holeY - f.shadowRadius, 2 * f.shadowRadius, 2 * f.shadowRadius));
        }
        if (f.alpha > 0.01f) {
            BufferedImage card = rounded(photo, 18 * photo.getWidth() / (W * 0.72f));
            java.awt.Composite old = g.getComposite();
            g.setComposite(AlphaComposite.getInstance(AlphaComposite.SRC_OVER, f.alpha));
            mesh(g, card, f.verts);
            g.setComposite(old);
        }
        hole(g, f, 1);
        g.dispose();
        return out;
    }

    static final float HOLE_X = W / 2f, HOLE_Y = 14, HOLE_R = 5;

    /** The camera hole, the thin ring turning around it, and the ripple: as WarpView draws them. */
    static void hole(Graphics2D g, Warp.Frame f, float dp) {
        float blue = 0x23 / 255f, g2 = 0x7E / 255f, b2 = 0xF0 / 255f;
        if (f.ringAlpha > 0) {
            g.setStroke(new BasicStroke(1.5f * dp));
            for (float k : new float[]{1f, 0.62f}) {
                g.setColor(new Color(blue, g2, b2, f.ringAlpha * k));
                float r = f.holeRadius + (f.ringRadius - f.holeRadius) * k;
                g.draw(new Ellipse2D.Float(f.holeX - r, f.holeY - r, 2 * r, 2 * r));
            }
        }
        if (f.haloAlpha > 0.01f) {
            float r = f.holeRadius * 1.6f + 2 * dp;
            g.setStroke(new BasicStroke(1.5f * dp, BasicStroke.CAP_ROUND, BasicStroke.JOIN_ROUND));
            g.setColor(new Color(blue, g2, b2, 0.3f * f.haloAlpha));
            g.draw(new Ellipse2D.Float(f.holeX - r, f.holeY - r, 2 * r, 2 * r));
            g.setColor(new Color(blue, g2, b2, 0.9f * f.haloAlpha));
            g.draw(new java.awt.geom.Arc2D.Float(f.holeX - r, f.holeY - r, 2 * r, 2 * r,
                    (float) -Math.toDegrees(f.haloSpin), 200, java.awt.geom.Arc2D.OPEN));
        }
        g.setColor(Color.BLACK);
        g.fill(new Ellipse2D.Float(f.holeX - f.holeRadius, f.holeY - f.holeRadius, 2 * f.holeRadius, 2 * f.holeRadius));
    }

    /** drawBitmapMesh, triangle by triangle. */
    static void mesh(Graphics2D g, BufferedImage img, float[] v) {
        int cols = Warp.COLS, rows = Warp.ROWS;
        float cw = img.getWidth() / (float) cols, ch = img.getHeight() / (float) rows;
        for (int r = 0; r < rows; r++) {
            for (int c = 0; c < cols; c++) {
                int a = (r * (cols + 1) + c) * 2, b = a + 2, d = ((r + 1) * (cols + 1) + c) * 2, e = d + 2;
                float sx = c * cw, sy = r * ch;
                tri(g, img, sx, sy, sx + cw, sy, sx, sy + ch, v[a], v[a + 1], v[b], v[b + 1], v[d], v[d + 1]);
                tri(g, img, sx + cw, sy, sx + cw, sy + ch, sx, sy + ch, v[b], v[b + 1], v[e], v[e + 1], v[d], v[d + 1]);
            }
        }
    }

    static void tri(Graphics2D g, BufferedImage img, float sx0, float sy0, float sx1, float sy1, float sx2, float sy2,
                    float dx0, float dy0, float dx1, float dy1, float dx2, float dy2) {
        double det = (sx1 - sx0) * (sy2 - sy0) - (sx2 - sx0) * (sy1 - sy0);
        if (Math.abs(det) < 1e-6) return;
        double a = ((dx1 - dx0) * (sy2 - sy0) - (dx2 - dx0) * (sy1 - sy0)) / det;
        double b = ((dx2 - dx0) * (sx1 - sx0) - (dx1 - dx0) * (sx2 - sx0)) / det;
        double c = ((dy1 - dy0) * (sy2 - sy0) - (dy2 - dy0) * (sy1 - sy0)) / det;
        double d = ((dy2 - dy0) * (sx1 - sx0) - (dy1 - dy0) * (sx2 - sx0)) / det;
        AffineTransform t = new AffineTransform(a, c, b, d, dx0 - a * sx0 - b * sy0, dy0 - c * sx0 - d * sy0);
        Path2D.Float clip = new Path2D.Float();
        // A hair larger, so neighbouring triangles leave no seams.
        float mx = (dx0 + dx1 + dx2) / 3, my = (dy0 + dy1 + dy2) / 3, k = 1.04f;
        clip.moveTo(mx + (dx0 - mx) * k, my + (dy0 - my) * k);
        clip.lineTo(mx + (dx1 - mx) * k, my + (dy1 - my) * k);
        clip.lineTo(mx + (dx2 - mx) * k, my + (dy2 - my) * k);
        clip.closePath();
        java.awt.Shape old = g.getClip();
        g.clip(clip);
        g.drawImage(img, t, null);
        g.setClip(old);
    }

    static BufferedImage rounded(BufferedImage src, float radius) {
        BufferedImage o = new BufferedImage(src.getWidth(), src.getHeight(), BufferedImage.TYPE_INT_ARGB);
        Graphics2D g = o.createGraphics();
        g.setRenderingHint(RenderingHints.KEY_ANTIALIASING, RenderingHints.VALUE_ANTIALIAS_ON);
        g.fill(new RoundRectangle2D.Float(0, 0, src.getWidth(), src.getHeight(), radius * 2, radius * 2));
        g.setComposite(AlphaComposite.SrcIn);
        g.drawImage(src, 0, 0, null);
        g.dispose();
        return o;
    }

    /** A stand-in photo: sky, sun, hills. */
    static BufferedImage photo(int w, int h) {
        BufferedImage p = new BufferedImage(w, h, BufferedImage.TYPE_INT_RGB);
        Graphics2D g = p.createGraphics();
        g.setRenderingHint(RenderingHints.KEY_ANTIALIASING, RenderingHints.VALUE_ANTIALIAS_ON);
        g.setPaint(new GradientPaint(0, 0, new Color(0xF6B26B), 0, h * 0.7f, new Color(0x7FA7D9)));
        g.fillRect(0, 0, w, h);
        g.setColor(new Color(0xFFF1C9));
        g.fillOval(w * 6 / 10, h / 5, w / 6, w / 6);
        g.setColor(new Color(0x3E6B4A));
        g.fillOval(-w / 4, h * 6 / 10, w, h);
        g.setColor(new Color(0x2C4F37));
        g.fillOval(w / 3, h * 7 / 10, w, h);
        g.dispose();
        return p;
    }

    /** A stand-in home screen: wallpaper and a grid of app icons. */
    static BufferedImage home() {
        BufferedImage p = new BufferedImage(W, H, BufferedImage.TYPE_INT_RGB);
        Graphics2D g = p.createGraphics();
        g.setRenderingHint(RenderingHints.KEY_ANTIALIASING, RenderingHints.VALUE_ANTIALIAS_ON);
        g.setPaint(new GradientPaint(0, 0, new Color(0x1D2B3A), W, H, new Color(0x3A5068)));
        g.fillRect(0, 0, W, H);
        Color[] icons = {new Color(0xE8EAED), new Color(0x34A853), new Color(0xFBBC04), new Color(0x4285F4), new Color(0xEA4335)};
        for (int r = 0; r < 5; r++) {
            for (int c = 0; c < 4; c++) {
                g.setColor(icons[(r * 4 + c) % icons.length]);
                g.fill(new RoundRectangle2D.Float(34 + c * 80, 380 + r * 76, 52, 52, 18, 18));
            }
        }
        g.setColor(new Color(255, 255, 255, 200));
        g.drawString("12:30", 18, 18);
        g.setColor(Color.BLACK);
        g.fill(new Ellipse2D.Float(HOLE_X - HOLE_R, HOLE_Y - HOLE_R, 2 * HOLE_R, 2 * HOLE_R));
        g.dispose();
        return p;
    }
}
