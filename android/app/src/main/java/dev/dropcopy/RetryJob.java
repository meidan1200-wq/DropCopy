package dev.dropcopy;

import android.app.job.JobInfo;
import android.app.job.JobParameters;
import android.app.job.JobScheduler;
import android.app.job.JobService;
import android.content.ComponentName;
import android.content.Context;
import java.io.IOException;

/** Sends the outbox when a network is up; retries every 30 s, 60 s, 90 s… while the PC is away. */
public class RetryJob extends JobService {
    private static final int ID = 7;

    static void schedule(Context c) {
        JobScheduler js = c.getSystemService(JobScheduler.class);
        if (js == null) return;
        js.schedule(new JobInfo.Builder(ID, new ComponentName(c, RetryJob.class))
                .setRequiredNetworkType(JobInfo.NETWORK_TYPE_ANY)
                .setBackoffCriteria(30_000, JobInfo.BACKOFF_POLICY_LINEAR)
                .setPersisted(true) // survives a reboot
                .build());
    }

    @Override
    public boolean onStartJob(JobParameters params) {
        new Thread(() -> {
            boolean again = false;
            try {
                int n = Outbox.flush(this);
                if (n > 0) Transfer.toast(this, "✓ Sent what was waiting (" + n + ")");
            } catch (Proto.Unreachable e) {
                again = true; // PC not reachable yet: back off and try again
            } catch (IOException e) {
                // Refused (e.g. re-paired PC): keep the files, stop retrying until the next send.
            }
            jobFinished(params, again);
        }, "outbox").start();
        return true;
    }

    @Override
    public boolean onStopJob(JobParameters params) {
        return true;
    }
}
