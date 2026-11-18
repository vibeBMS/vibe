package io.github.vibebms;

import android.app.Activity;
import android.os.Build;
import android.view.View;
import android.view.Window;
import android.view.WindowInsets;
import android.view.WindowInsetsController;

/**
 * The game on the whole screen: the status and navigation bars hidden
 * (immersive: a swipe from an edge shows them for a moment, over the game).
 * A display cutout keeps the system's default (the picture beside it in
 * landscape, so no lane is under the camera). Called from Rust through JNI
 * (crates/vibebms/src/platform/android.rs) at start and whenever the app
 * gets the focus back, as the system may show the bars again meanwhile.
 */
public final class SystemBars {
    private SystemBars() {}

    @SuppressWarnings("deprecation") // the flags before API 30
    public static void hide(final Activity activity) {
        activity.runOnUiThread(() -> {
            Window window = activity.getWindow();
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                window.setDecorFitsSystemWindows(false);
                WindowInsetsController c = window.getInsetsController();
                if (c != null) {
                    c.hide(WindowInsets.Type.systemBars());
                    c.setSystemBarsBehavior(
                            WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE);
                }
            } else {
                window.getDecorView().setSystemUiVisibility(
                        View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
                                | View.SYSTEM_UI_FLAG_FULLSCREEN
                                | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                                | View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                                | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                                | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN);
            }
        });
    }
}
