package dev.dropcopy;

import static org.junit.Assume.assumeTrue;

import android.graphics.Bitmap;
import android.graphics.Canvas;
import android.view.View;
import java.io.File;
import java.io.FileOutputStream;
import org.junit.Test;
import org.junit.runner.RunWith;
import org.robolectric.Robolectric;
import org.robolectric.RobolectricTestRunner;
import org.robolectric.RuntimeEnvironment;
import org.robolectric.annotation.Config;
import org.robolectric.annotation.GraphicsMode;

/**
 * Renders the main screen with Android's real graphics stack (Robolectric native graphics) and
 * writes PNGs to $DROPCOPY_SHOTS, for looking at the design without a device.
 */
@RunWith(RobolectricTestRunner.class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = 35, qualifiers = "w411dp-h891dp-xxhdpi")
public class ScreenshotTest {
    private static final String LINK = "dropcopy://pair?v=1&id=0011223344556677"
            + "&k=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8&n=DESKTOP-4E1E3TB&p=48650&a=192.168.1.120";

    private void shoot(String name) throws Exception {
        String dir = System.getenv("DROPCOPY_SHOTS");
        assumeTrue("set DROPCOPY_SHOTS to write screenshots", dir != null && !dir.isEmpty());
        MainActivity a = Robolectric.buildActivity(MainActivity.class).setup().get();
        View root = a.getWindow().getDecorView();
        View content = a.findViewById(R.id.scroll);
        // Measure the whole scrollable page, not just the first screenful.
        int w = root.getWidth() > 0 ? root.getWidth() : 1233;
        View page = ((android.view.ViewGroup) content).getChildAt(0);
        page.measure(View.MeasureSpec.makeMeasureSpec(w, View.MeasureSpec.EXACTLY), View.MeasureSpec.makeMeasureSpec(0, View.MeasureSpec.UNSPECIFIED));
        page.layout(0, 0, w, page.getMeasuredHeight());
        Bitmap bmp = Bitmap.createBitmap(w, page.getMeasuredHeight(), Bitmap.Config.ARGB_8888);
        Canvas c = new Canvas(bmp);
        c.drawColor(a.getColor(R.color.bg));
        page.draw(c);
        try (FileOutputStream out = new FileOutputStream(new File(dir, name + ".png"))) {
            bmp.compress(Bitmap.CompressFormat.PNG, 100, out);
        }
    }

    @Test
    public void unpairedLight() throws Exception {
        shoot("main-unpaired-light");
    }

    @Test
    public void pairedLight() throws Exception {
        Store.add(RuntimeEnvironment.getApplication(), LINK);
        Store.setFlag(RuntimeEnvironment.getApplication(), Store.WATCH_SCREENSHOTS, true);
        shoot("main-paired-light");
    }

    @Test
    @Config(qualifiers = "w411dp-h891dp-night-xxhdpi")
    public void pairedDark() throws Exception {
        Store.add(RuntimeEnvironment.getApplication(), LINK);
        Store.setFlag(RuntimeEnvironment.getApplication(), Store.NEAR, true);
        shoot("main-paired-dark");
    }
}
