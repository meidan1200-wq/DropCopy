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

    /**
     * A random id made once per install: PCs tell apart two phones with the same name by it
     * ("Galaxy S20+" and "Galaxy S20+"), so one never collects what was sent to the other.
     */
    static synchronized String phoneId(Context c) {
        String id = prefs(c).getString("phone_id", null);
        if (id == null) {
            byte[] b = new byte[8];
            new java.security.SecureRandom().nextBytes(b);
            StringBuilder sb = new StringBuilder();
            for (byte x : b) sb.append(String.format("%02x", x & 0xff));
            id = sb.toString();
            prefs(c).edit().putString("phone_id", id).apply();
        }
        return id;
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

    /** The one share-sheet icon in the top row ("Direct Share"); also on a long press of the app icon. */
    static final String AUTO_SHORTCUT = "auto";

    /**
     * One icon however many PCs are paired. Tapping it sends to the right one: the PC the phone
     * is held next to, else the one someone is using, else the last one used ({@link Proto#choose}).
     */
    static void publishShortcuts(Context c, List<Proto.Pc> pcs) {
        ShortcutManager sm = c.getSystemService(ShortcutManager.class);
        if (sm == null) return;
        List<ShortcutInfo> list = new ArrayList<>();
        if (!pcs.isEmpty()) {
            String label = pcs.size() == 1 ? pcs.get(0).name : "My PC";
            Intent clip = new Intent(Intent.ACTION_VIEW).setClass(c, ClipActivity.class);
            ShortcutInfo.Builder b = new ShortcutInfo.Builder(c, AUTO_SHORTCUT)
                    .setShortLabel(label)
                    .setLongLabel(pcs.size() == 1 ? "Send to " + label : "Send to my PC")
                    .setIcon(Icon.createWithResource(c, R.mipmap.ic_launcher))
                    .setCategories(Collections.singleton(CATEGORY))
                    .setIntent(clip);
            if (Build.VERSION.SDK_INT >= 30) b.setLongLived(true);
            list.add(b.build());
        }
        try {
            // Replaces the old one-icon-per-PC shortcuts, so the share sheet shows a single icon.
            sm.setDynamicShortcuts(list);
        } catch (IllegalStateException | IllegalArgumentException ignored) {
            // Rate-limited or rejected: the plain share target still works.
        }
        if (Build.VERSION.SDK_INT >= 30) {
            List<String> old = new ArrayList<>();
            for (Proto.Pc pc : pcs) old.add("pc-" + pc.hexId());
            try { sm.removeLongLivedShortcuts(old); } catch (RuntimeException ignored) { }
        }
    }

    /** Receive from your PCs: on unless turned off. */
    static boolean receive(Context c) {
        return prefs(c).getBoolean(RECEIVE, true);
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
    static final String RECEIVE = "receive";
}
