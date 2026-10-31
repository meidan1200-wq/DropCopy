package dev.dropcopy;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageInstaller;

/** Result of an install session. */
public class UpdateReceiver extends BroadcastReceiver {
    @Override
    @SuppressWarnings("deprecation")
    public void onReceive(Context c, Intent intent) {
        int status = intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE);
        if (status == PackageInstaller.STATUS_PENDING_USER_ACTION) {
            Intent confirm = intent.getParcelableExtra(Intent.EXTRA_INTENT);
            if (confirm == null) return;
            // The user just tapped Install in the app: show Android's install screen right away.
            // Only from the background (the daily check) does it wait in a notification.
            if (MainActivity.visible()) {
                try {
                    c.startActivity(confirm.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
                    return;
                } catch (RuntimeException ignored) { }
            }
            Updater.askToConfirm(c, confirm);
        } else if (status != PackageInstaller.STATUS_SUCCESS) {
            String msg = intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE);
            if (status != PackageInstaller.STATUS_FAILURE_ABORTED) Transfer.toast(c, "Update not installed" + (msg != null ? ": " + msg : ""));
            MainActivity.installEnded();
        }
        // On success the app is replaced and restarted by the system; BootReceiver's
        // MY_PACKAGE_REPLACED brings the background services back.
    }
}
