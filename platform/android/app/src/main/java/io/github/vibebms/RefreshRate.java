package io.github.vibebms;

import android.app.Activity;
import android.view.Display;
import android.view.Window;
import android.view.WindowManager;

/**
 * The display's highest refresh rate for the game: phones and tablets with
 * 90/120/144 Hz screens show an app at 60 Hz unless it asks for more. Called
 * from Rust through JNI (crates/vibebms/src/platform/android.rs).
 */
public final class RefreshRate {
    private RefreshRate() {}

    /**
     * Ask for the display mode of the current resolution with the highest
     * refresh rate; that rate (the system may still lower it, in a power
     * saving mode for one).
     */
    @SuppressWarnings("deprecation") // getDefaultDisplay: Activity.getDisplay needs API 30
    public static float useHighest(final Activity activity) {
        Display display = activity.getWindowManager().getDefaultDisplay();
        Display.Mode current = display.getMode();
        Display.Mode best = current;
        for (Display.Mode m : display.getSupportedModes()) {
            if (m.getPhysicalWidth() == current.getPhysicalWidth()
                    && m.getPhysicalHeight() == current.getPhysicalHeight()
                    && m.getRefreshRate() > best.getRefreshRate()) {
                best = m;
            }
        }
        final int id = best.getModeId();
        activity.runOnUiThread(() -> {
            Window window = activity.getWindow();
            WindowManager.LayoutParams lp = window.getAttributes();
            if (lp.preferredDisplayModeId != id) {
                lp.preferredDisplayModeId = id;
                window.setAttributes(lp);
            }
        });
        return best.getRefreshRate();
    }
}
