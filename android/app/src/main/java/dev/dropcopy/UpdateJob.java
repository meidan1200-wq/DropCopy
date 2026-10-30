package dev.dropcopy;

import android.app.job.JobParameters;
import android.app.job.JobService;

/** The daily background update check. */
public class UpdateJob extends JobService {
    @Override
    public boolean onStartJob(JobParameters params) {
        new Thread(() -> {
            try {
                Updater.run(this);
            } catch (Exception ignored) {
                // Offline or GitHub unreachable: tomorrow's run tries again.
            }
            jobFinished(params, false);
        }, "update").start();
        return true;
    }

    @Override
    public boolean onStopJob(JobParameters params) {
        return true;
    }
}
