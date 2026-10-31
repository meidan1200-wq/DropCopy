package dev.dropcopy;

import android.content.Context;
import android.content.Intent;
import android.content.SharedPreferences;
import android.content.pm.ShortcutInfo;
import android.content.pm.ShortcutManager;
import android.graphics.drawable.Icon;
import android.os.Build;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;

/** Paired PCs and settings. The first PC in the list is the most recently used one. */
final class Store {
    private static final String PREFS = "dropcopy";
    static final String CATEGORY = "dev.dropcopy.category.PC";

    private Store() {}

    private static SharedPreferences prefs(Context c) {
        return c.getSharedPreferences(PREFS, Context.MODE_PRIVATE);
    }

    static synchronized List<Proto.Pc> pcs(Context c) {
        List<Proto.Pc> out = new ArrayList<>();
        for (String line : prefs(c).getString("pcs", "").split("\n")) {
            if (line.isEmpty()) continue;
            try { out.add(Proto.Pc.parse(line)); } catch (IllegalArgumentException ignored) { }
        }
        return out;
    }

    private static void save(Context c, List<Proto.Pc> pcs) {
        StringBuilder sb = new StringBuilder();
        for (Proto.Pc pc : pcs) sb.append(pc.toLink()).append('\n');
        prefs(c).edit().putString("pcs", sb.toString()).apply();
        publishShortcuts(c, pcs);
    }

    /** Pairs from a link; re-pairing the same PC replaces it. */
    static synchronized Proto.Pc add(Context c, String link) {
        Proto.Pc pc = Proto.Pc.parse(link);
        List<Proto.Pc> pcs = pcs(c);
        pcs.removeIf(p -> p.hexId().equals(pc.hexId()));
        pcs.add(0, pc);
        save(c, pcs);
        return pc;
    }

    /** Records where a PC was just reached and makes it the default. */
    static synchronized void remember(Context c, Proto.Pc used) {
        List<Proto.Pc> pcs = pcs(c);
        pcs.removeIf(p -> p.hexId().equals(used.hexId()));
        pcs.add(0, used);
        save(c, pcs);
    }

    static synchronized void forgetAll(Context c) {
        save(c, Collections.emptyList());
    }

    /** One share-sheet icon per PC ("Direct Share"), also shown on a long press of the app icon. */
    static void publishShortcuts(Context c, List<Proto.Pc> pcs) {
        ShortcutManager sm = c.getSystemService(ShortcutManager.class);
        if (sm == null) return;
        List<ShortcutInfo> list = new ArrayList<>();
        int max = Math.min(4, sm.getMaxShortcutCountPerActivity());
        for (Proto.Pc pc : pcs) {
            if (list.size() >= max) break;
            Intent clip = new Intent(Intent.ACTION_VIEW).setClass(c, ClipActivity.class).putExtra("pc", pc.hexId());
            ShortcutInfo.Builder b = new ShortcutInfo.Builder(c, "pc-" + pc.hexId())
                    .setShortLabel(pc.name)
                    .setLongLabel("Send clipboard to " + pc.name)
                    .setIcon(Icon.createWithResource(c, R.mipmap.ic_launcher))
                    .setCategories(Collections.singleton(CATEGORY))
                    .setIntent(clip);
            if (Build.VERSION.SDK_INT >= 30) b.setLongLived(true);
            list.add(b.build());
        }
        try {
            sm.setDynamicShortcuts(list);
        } catch (IllegalStateException | IllegalArgumentException ignored) {
            // Rate-limited or rejected: the plain share target still works.
        }
    }

    static boolean flag(Context c, String key) {
        return prefs(c).getBoolean(key, false);
    }

    static void setFlag(Context c, String key, boolean v) {
        prefs(c).edit().putBoolean(key, v).apply();
    }

    static boolean autoUpdate(Context c) {
        return prefs(c).getBoolean(AUTO_UPDATE, true);
    }

    static final String AUTO_UPDATE = "auto_update";
    static final String NEAR = "near";
    /** Pictures are deleted from the PC once pasted (or when something else is copied there). */
    static final String DELETE_AFTER_PASTE = "delete_after_paste";
    static final String WATCH_SCREENSHOTS = "watch_screenshots";
    static final String WATCH_CAMERA = "watch_camera";
}
