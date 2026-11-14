package io.github.vibebms;

import android.app.Activity;
import android.app.Dialog;
import android.graphics.Color;
import android.view.Gravity;
import android.view.ViewGroup;
import android.webkit.CookieManager;
import android.webkit.URLUtil;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.TextView;

import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.util.concurrent.ConcurrentLinkedQueue;

/**
 * The download browser: a full-screen web view over the game. What a page
 * downloads is saved to the given folder without asking; the game polls
 * {@link #poll()} for what happened and brings finished files into the
 * library. Called from Rust through JNI (crates/vibebms/src/browser_android.rs).
 */
public final class Browser {
    /** "S" + file name (started), "F" + path (finished), "E" + message, "C" (closed). */
    private static final ConcurrentLinkedQueue<String> EVENTS = new ConcurrentLinkedQueue<>();
    private static Dialog dialog;
    private static WebView web;
    private static TextView title;

    private Browser() {}

    /** Show {@code url}, opening the browser if needed; downloads go to {@code dir}. */
    public static void open(final Activity activity, final String url, final String dir) {
        activity.runOnUiThread(() -> {
            if (dialog == null) {
                build(activity, dir);
            }
            web.loadUrl(url);
            dialog.show();
        });
    }

    /** The next event, or null. */
    public static String poll() {
        return EVENTS.poll();
    }

    private static void build(final Activity activity, final String dir) {
        dialog = new Dialog(activity, android.R.style.Theme_DeviceDefault_NoActionBar_Fullscreen);
        LinearLayout root = new LinearLayout(activity);
        root.setOrientation(LinearLayout.VERTICAL);

        LinearLayout bar = new LinearLayout(activity);
        bar.setOrientation(LinearLayout.HORIZONTAL);
        bar.setGravity(Gravity.CENTER_VERTICAL);
        bar.setBackgroundColor(Color.rgb(20, 24, 36));
        int pad = (int) (8 * activity.getResources().getDisplayMetrics().density);
        bar.setPadding(pad, pad / 2, pad, pad / 2);
        Button back = new Button(activity);
        back.setText("◀");
        back.setOnClickListener(v -> {
            if (web.canGoBack()) {
                web.goBack();
            }
        });
        title = new TextView(activity);
        title.setTextColor(Color.rgb(226, 232, 245));
        title.setSingleLine(true);
        title.setPadding(pad, 0, pad, 0);
        Button close = new Button(activity);
        close.setText("Close");
        close.setOnClickListener(v -> dialog.dismiss());
        bar.addView(back);
        bar.addView(title, new LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f));
        bar.addView(close);

        web = new WebView(activity);
        WebSettings s = web.getSettings();
        s.setJavaScriptEnabled(true);
        s.setDomStorageEnabled(true);
        s.setSupportMultipleWindows(false); // target=_blank opens here
        web.setWebViewClient(new WebViewClient() {
            @Override
            public void onPageFinished(WebView view, String url) {
                title.setText(view.getTitle() != null ? view.getTitle() : url);
            }
        });
        web.setDownloadListener((url, userAgent, disposition, mime, length) ->
                download(url, userAgent, disposition, mime, new File(dir)));

        root.addView(bar, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));
        root.addView(web, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f));
        dialog.setContentView(root);
        dialog.setOnDismissListener(d -> EVENTS.add("C"));
        dialog.setOnKeyListener((d, code, ev) -> {
            if (code == android.view.KeyEvent.KEYCODE_BACK
                    && ev.getAction() == android.view.KeyEvent.ACTION_UP && web.canGoBack()) {
                web.goBack();
                return true;
            }
            return false;
        });
    }

    /** Save a download in the background, with the page's cookies. */
    private static void download(final String url, final String userAgent, final String disposition,
                                 final String mime, final File dir) {
        if (!url.startsWith("http://") && !url.startsWith("https://")) {
            EVENTS.add("EThis download cannot be saved (" + url.split(":")[0] + " link)");
            return;
        }
        final String name = URLUtil.guessFileName(url, disposition, mime).replaceAll("[/\\\\:]", "_");
        final String cookies = CookieManager.getInstance().getCookie(url);
        EVENTS.add("S" + name);
        new Thread(() -> {
            try {
                dir.mkdirs();
                File out = free(dir, name);
                HttpURLConnection c = open(url, userAgent, cookies);
                try (InputStream in = c.getInputStream(); OutputStream os = new FileOutputStream(out)) {
                    byte[] buf = new byte[1 << 16];
                    for (int n; (n = in.read(buf)) > 0; ) {
                        os.write(buf, 0, n);
                    }
                } finally {
                    c.disconnect();
                }
                EVENTS.add("F" + out.getAbsolutePath());
            } catch (Exception e) {
                EVENTS.add("E" + name + ": " + e.getMessage());
            }
        }, "download").start();
    }

    /** A connection to {@code url}, following redirects (also between http and https). */
    private static HttpURLConnection open(String url, String userAgent, String cookies) throws Exception {
        for (int i = 0; i < 8; i++) {
            HttpURLConnection c = (HttpURLConnection) new URL(url).openConnection();
            c.setInstanceFollowRedirects(false);
            c.setRequestProperty("User-Agent", userAgent);
            if (cookies != null) {
                c.setRequestProperty("Cookie", cookies);
            }
            int code = c.getResponseCode();
            if (code / 100 == 3 && c.getHeaderField("Location") != null) {
                url = new URL(new URL(url), c.getHeaderField("Location")).toString();
                c.disconnect();
                continue;
            }
            if (code / 100 != 2) {
                c.disconnect();
                throw new Exception("HTTP " + code);
            }
            return c;
        }
        throw new Exception("too many redirects");
    }

    /** {@code dir/name}, or {@code name (2)} etc. when that exists. */
    private static File free(File dir, String name) {
        File f = new File(dir, name);
        int dot = name.lastIndexOf('.');
        String stem = dot > 0 ? name.substring(0, dot) : name;
        String ext = dot > 0 ? name.substring(dot) : "";
        for (int k = 2; f.exists(); k++) {
            f = new File(dir, stem + " (" + k + ")" + ext);
        }
        return f;
    }
}
