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
        prefs(c).edit().remove("phones").apply();
        publishShortcuts(c, Collections.emptyList());
    }

    // ---- other phones -------------------------------------------------------------------------

    /** Phones this one can send to, most recently used first. */
    static synchronized List<Proto.Pc> phones(Context c) {
        List<Proto.Pc> out = new ArrayList<>();
        for (String line : prefs(c).getString("phones", "").split("\n")) {
            if (line.isEmpty()) continue;
            try {
                Proto.Pc p = Proto.Pc.parse(line);
                p.phone = true;
                out.add(p);
            } catch (IllegalArgumentException ignored) { }
        }
        return out;
    }

    /** Remembers a phone (scanned, or one that scanned this one), or where it was just reached. */
    static synchronized void rememberPhone(Context c, Proto.Pc phone) {
        phone.phone = true;
        if (java.util.Arrays.equals(phone.id, me(c).id)) return; // its own code
        List<Proto.Pc> list = phones(c);
        list.removeIf(p -> p.hexId().equals(phone.hexId()));
        list.add(0, phone);
        StringBuilder sb = new StringBuilder();
        for (Proto.Pc p : list.subList(0, Math.min(list.size(), 6))) sb.append(p.toLink()).append('\n');
        prefs(c).edit().putString("phones", sb.toString()).apply();
        publishShortcuts(c, pcs(c));
    }

    static synchronized void forgetPhone(Context c, String hex) {
        List<Proto.Pc> list = phones(c);
        list.removeIf(p -> p.hexId().equals(hex));
        StringBuilder sb = new StringBuilder();
        for (Proto.Pc p : list) sb.append(p.toLink()).append('\n');
        prefs(c).edit().putString("phones", sb.toString()).apply();
        publishShortcuts(c, pcs(c));
        ShortcutManager sm = c.getSystemService(ShortcutManager.class);
        if (sm != null && Build.VERSION.SDK_INT >= 30) {
            try { sm.removeLongLivedShortcuts(Collections.singletonList(PHONE_SHORTCUT + hex)); } catch (RuntimeException ignored) { }
        }
    }

    /**
     * This phone as a receiver: an id and key made once, like a PC's. Its link (the code it shows)
     * lets another phone send to it; it carries the key, so it is only ever shown on this screen.
     */
    static synchronized Proto.Pc me(Context c) {
        String id = prefs(c).getString("me_id", null), key = prefs(c).getString("me_key", null);
        if (id == null || key == null) {
            java.security.SecureRandom r = new java.security.SecureRandom();
            byte[] i = new byte[8], k = new byte[32];
            r.nextBytes(i);
            r.nextBytes(k);
            StringBuilder sb = new StringBuilder();
            for (byte x : i) sb.append(String.format("%02x", x & 0xff));
            id = sb.toString();
            key = java.util.Base64.getUrlEncoder().withoutPadding().encodeToString(k);
            prefs(c).edit().putString("me_id", id).putString("me_key", key).apply();
        }
        Proto.Pc me = Proto.Pc.parse("dropcopy://pair?v=1&id=" + id + "&k=" + key + "&n=" + Proto.enc(Transfer.deviceName(c))
                + "&p=" + PhoneServer.port());
        me.host = PcSetup.localAddress();
        me.phone = true;
        me.v2 = true;
        return me;
    }

    /** The one share-sheet icon in the top row ("Direct Share"); also on a long press of the app icon. */
    static final String AUTO_SHORTCUT = "auto";
    /** A paired phone's share-sheet icon: "phone-" + its id. */
    static final String PHONE_SHORTCUT = "phone-";

    /**
     * One icon however many PCs are paired. Tapping it sends to the right one: the PC the phone
     * is held next to, else the one someone is using, else the last one used ({@link Proto#choose}).
     */
    static void publishShortcuts(Context c, List<Proto.Pc> pcs) {
        ShortcutManager sm = c.getSystemService(ShortcutManager.class);
        if (sm == null) return;
        List<ShortcutInfo> list = new ArrayList<>();
        // Each paired phone has its own icon: a phone is a person, not "whichever PC is in use".
        for (Proto.Pc phone : phones(c)) {
            if (list.size() >= 3) break; // the share sheet shows a row of four: leave the PC its place
            ShortcutInfo.Builder b = new ShortcutInfo.Builder(c, PHONE_SHORTCUT + phone.hexId())
                    .setShortLabel(phone.name)
                    .setLongLabel("Send to " + phone.name)
                    .setIcon(Icon.createWithResource(c, R.drawable.ic_phone_share))
                    .setCategories(Collections.singleton(CATEGORY))
                    .setRank(1 + list.size())
                    .setIntent(new Intent(Intent.ACTION_VIEW).setClass(c, MainActivity.class));
            if (Build.VERSION.SDK_INT >= 30) b.setLongLived(true);
            list.add(b.build());
        }
        if (!pcs.isEmpty()) {
            String label = pcs.size() == 1 ? pcs.get(0).name : "My PC";
            Intent clip = new Intent(Intent.ACTION_VIEW).setClass(c, ClipActivity.class);
            ShortcutInfo.Builder b = new ShortcutInfo.Builder(c, AUTO_SHORTCUT)
                    .setShortLabel(label)
                    .setLongLabel(pcs.size() == 1 ? "Send to " + label : "Send to my PC")
                    .setIcon(Icon.createWithResource(c, R.mipmap.ic_launcher))
                    .setCategories(Collections.singleton(CATEGORY))
                    .setRank(0)
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
