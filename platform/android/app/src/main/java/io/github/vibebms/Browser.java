package io.github.vibebms;

import android.app.Activity;
import android.app.AlertDialog;
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
import java.util.Locale;
import java.util.concurrent.ConcurrentLinkedQueue;
import java.util.concurrent.atomic.AtomicBoolean;

/**
 * The download browser: a full-screen web view over the game. What a page
 * downloads is saved to the given folder once the player has said yes to it,
 * one file at a time and none over the size limit; the game polls
 * {@link #poll()} for what happened and brings finished files into the
 * library. Called from Rust through JNI (crates/vibebms/src/browser/android.rs).
 */
public final class Browser {
    /**
     * "S" + file name (started), "F" + path (finished), "E" + message, "B" + file name (not
     * started: another is under way), "L" + file name (over the size limit), "C" (closed).
     */
    private static final ConcurrentLinkedQueue<String> EVENTS = new ConcurrentLinkedQueue<>();
    /** A download is asked about or being saved. */
    private static final AtomicBoolean BUSY = new AtomicBoolean();
    private static Dialog dialog;
    private static WebView web;
    private static TextView title;

    private Browser() {}

    /**
     * Show {@code url}, opening the browser if needed; downloads of at most {@code max} bytes
     * go to {@code dir}, each after a question titled {@code ask} with the button {@code no}
     * beside OK. {@code script} runs on every page once it has loaded.
     */
    public static void open(final Activity activity, final String url, final String dir,
            final String script, final long max, final String ask, final String no) {
        activity.runOnUiThread(() -> {
            if (dialog == null) {
                build(activity, dir, script, max, ask, no);
            }
            web.loadUrl(url);
            dialog.show();
        });
    }

    /** The next event, or null. */
    public static String poll() {
        return EVENTS.poll();
    }

    private static void build(final Activity activity, final String dir, final String script,
            final long max, final String ask, final String no) {
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
                view.evaluateJavascript(script, null);
            }
        });
        web.setDownloadListener((url, userAgent, disposition, mime, length) ->
                ask(activity, url, userAgent, disposition, mime, length, new File(dir), max, ask,
                        no));

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

    /**
     * A page wants to save a file. Any page can, by itself: the player is asked first, with
     * the file's name, the host it comes from and its size when the server says it.
     */
    private static void ask(final Activity activity, final String url, final String userAgent,
            String disposition, String mime, long length, final File dir, final long max,
            String ask, String no) {
        String host;
        try {
            host = new URL(url).getHost();
        } catch (Exception e) {
            host = null;
        }
        if (host == null || !(url.startsWith("http://") || url.startsWith("https://"))) {
            EVENTS.add("EThis download cannot be saved (" + url.split(":")[0] + " link)");
            return;
        }
        final String name = safeName(URLUtil.guessFileName(url, disposition, mime));
        if (length > max) {
            EVENTS.add("L" + name);
            return;
        }
        if (!BUSY.compareAndSet(false, true)) {
            EVENTS.add("B" + name);
            return;
        }
        String size = length > 0
                ? String.format(Locale.US, "\n%.1f MB", length / 1048576.0) : "";
        final boolean[] yes = {false};
        new AlertDialog.Builder(activity)
                .setTitle(ask)
                .setMessage(name + "\n" + host + size)
                .setPositiveButton(android.R.string.ok, (d, which) -> {
                    yes[0] = true;
                    download(url, userAgent, name, dir, max);
                })
                .setNegativeButton(no, null)
                .setOnDismissListener(d -> {
                    if (!yes[0]) {
                        BUSY.set(false);
                    }
                })
                .show();
    }

    /**
     * {@code name} (the server's choice) as one file name: no path, nothing a file system
     * refuses, not hidden, of a length one takes.
     */
    static String safeName(String name) {
        String n = name.replaceAll("[/\\\\:*?\"<>|\\p{Cntrl}]", "_").trim()
                .replaceAll("^\\.+|\\.+$", "");
        if (n.isEmpty()) {
            return "download";
        }
        int dot = n.lastIndexOf('.');
        String ext = dot > 0 && n.length() - dot <= 16 ? n.substring(dot) : "";
        int keep = 80 - ext.length();
        return n.length() <= 80 ? n : n.substring(0, keep) + ext;
    }

    /** The download is larger than a download may be. */
    private static final class TooLarge extends Exception {}

    /** Save a download in the background, {@code max} bytes at most. */
    private static void download(final String url, final String userAgent, final String name,
            final File dir, final long max) {
        EVENTS.add("S" + name);
        new Thread(() -> {
            File out = null;
            try {
                dir.mkdirs();
                out = free(dir, name);
                HttpURLConnection c = open(url, userAgent);
                try (InputStream in = c.getInputStream(); OutputStream os = new FileOutputStream(out)) {
                    if (c.getContentLengthLong() > max) {
                        throw new TooLarge();
                    }
                    byte[] buf = new byte[1 << 16];
                    long total = 0;
                    for (int n; (n = in.read(buf)) > 0; ) {
                        total += n;
                        if (total > max) {
                            throw new TooLarge();
                        }
                        os.write(buf, 0, n);
                    }
                } finally {
                    c.disconnect();
                }
                EVENTS.add("F" + out.getAbsolutePath());
            } catch (Exception e) {
                if (out != null) {
                    out.delete();
                }
                EVENTS.add(e instanceof TooLarge ? "L" + name : "E" + name + ": " + e.getMessage());
            } finally {
                BUSY.set(false);
            }
        }, "download").start();
    }

    /**
     * A connection to {@code url}, following redirects by hand. Each hop gets the cookies the
     * web view keeps for that hop's own address, and only over https; a hop never leaves https
     * for http.
     */
    private static HttpURLConnection open(String url, String userAgent) throws Exception {
        for (int i = 0; i < 8; i++) {
            boolean secure = url.startsWith("https://");
            HttpURLConnection c = (HttpURLConnection) new URL(url).openConnection();
            c.setInstanceFollowRedirects(false);
            c.setConnectTimeout(15000);
            c.setReadTimeout(30000);
            c.setRequestProperty("User-Agent", userAgent);
            String cookies = secure ? CookieManager.getInstance().getCookie(url) : null;
            if (cookies != null) {
                c.setRequestProperty("Cookie", cookies);
            }
            int code = c.getResponseCode();
            if (code / 100 == 3 && c.getHeaderField("Location") != null) {
                String next = new URL(new URL(url), c.getHeaderField("Location")).toString();
                c.disconnect();
                if (!(next.startsWith("https://") || (!secure && next.startsWith("http://")))) {
                    throw new Exception("redirect refused");
                }
                url = next;
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
