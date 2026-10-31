package dev.dropcopy;

import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.net.Uri;
import android.os.Build;
import android.provider.Settings;
import java.util.Locale;

/**
 * Phone makers whose own battery manager stops background apps beyond what Android does, and the
 * screen where the user lets an app run. Android's battery exemption is not enough on these: the
 * "auto-start" (or "app launch") switch is separate. Screens differ between versions, so each
 * maker has several candidates and only one that exists on this phone is offered.
 */
final class Oem {
    private Oem() {}

    private static final String[][] SCREENS = {
            // brand keywords, package, activity
            {"xiaomi redmi poco", "com.miui.securitycenter", "com.miui.permcenter.autostart.AutoStartManagementActivity"},
            {"huawei honor", "com.huawei.systemmanager", "com.huawei.systemmanager.startupmgr.ui.StartupNormalAppListActivity"},
            {"huawei honor", "com.huawei.systemmanager", "com.huawei.systemmanager.optimize.process.ProtectActivity"},
            {"honor", "com.hihonor.systemmanager", "com.hihonor.systemmanager.startupmgr.ui.StartupNormalAppListActivity"},
            {"oppo realme oneplus", "com.coloros.safecenter", "com.coloros.safecenter.permission.startup.StartupAppListActivity"},
            {"oppo realme oneplus", "com.coloros.safecenter", "com.coloros.safecenter.startupapp.StartupAppListActivity"},
            {"oppo realme oneplus", "com.oplus.safecenter", "com.oplus.safecenter.startupapp.StartupAppListActivity"},
            {"oneplus", "com.oneplus.security", "com.oneplus.security.chainlaunch.view.ChainLaunchAppListActivity"},
            {"vivo iqoo", "com.vivo.permissionmanager", "com.vivo.permissionmanager.activity.BgStartUpManagerActivity"},
            {"vivo iqoo", "com.iqoo.secure", "com.iqoo.secure.ui.phoneoptimize.BgStartUpManager"},
            {"asus", "com.asus.mobilemanager", "com.asus.mobilemanager.autostart.AutoStartActivity"},
    };

    static String maker() {
        return (Build.MANUFACTURER == null ? "" : Build.MANUFACTURER).toLowerCase(Locale.ROOT).trim();
    }

    /** The auto-start screen on this phone, or null when the maker has none (Samsung, Pixel, …). */
    static Intent autostart(Context c) {
        String m = maker();
        if (m.isEmpty()) return null;
        PackageManager pm = c.getPackageManager();
        for (String[] s : SCREENS) {
            if (!(" " + s[0] + " ").contains(" " + m + " ")) continue;
            Intent i = new Intent().setComponent(new ComponentName(s[1], s[2])).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
            if (pm.resolveActivity(i, 0) != null) return i;
        }
        return null;
    }

    /** "Xiaomi", "Oneplus": for the sentence that names the maker. */
    static String brandName() {
        String m = maker();
        return m.isEmpty() ? "This phone" : Character.toUpperCase(m.charAt(0)) + m.substring(1);
    }

    /** Opens the screen; if the maker blocks it, the app's own settings page instead. */
    static void open(Context c, Intent screen) {
        try {
            c.startActivity(screen);
        } catch (RuntimeException e) {
            c.startActivity(new Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:" + c.getPackageName()))
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
        }
    }
}
