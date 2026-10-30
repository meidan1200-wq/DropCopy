package dev.dropcopy;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;

/** Restarts auto-send after a reboot or an app update. */
public class BootReceiver extends BroadcastReceiver {
    @Override
    public void onReceive(Context c, Intent intent) {
        String a = intent.getAction();
        if (Intent.ACTION_BOOT_COMPLETED.equals(a) || Intent.ACTION_MY_PACKAGE_REPLACED.equals(a)) WatchService.sync(c);
    }
}
