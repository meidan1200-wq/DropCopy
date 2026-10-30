package dev.dropcopy;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;

/** A network came up while something is queued: try right away instead of waiting for backoff. */
public class NetReceiver extends BroadcastReceiver {
    @Override
    public void onReceive(Context c, Intent intent) {
        if (!Outbox.isEmpty(c)) RetryJob.schedule(c);
    }
}
