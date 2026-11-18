//! The selected chart's files and links: its folder in the system's file
//! manager (F3, desktop), its web page (U: the chart's `#URL`, else the
//! difficulty table's link for it) and the other charts of its folder (B:
//! SAME FOLDER).

use super::status::song_dir;
use super::tree::{Bar, Node};
use super::SelectScene;
use crate::app::Ctx;
use crate::library::Song;
use crate::platform::{open, vfs};
use std::path::Path;

/// A link that may be opened: http(s) only (a chart's `#URL` is the
/// author's text).
fn web_link(s: &str) -> Option<String> {
    let s = s.trim();
    let lower = s.to_ascii_lowercase();
    (lower.starts_with("http://") || lower.starts_with("https://"))
        .then(|| s.to_string())
        .filter(|s| s.len() > 8 && !s.chars().any(char::is_whitespace))
}

/// The web page a chart's file names: a BMS `#URL` header, a bmson
/// `info.url` (not in the bmson spec, but some converters write one).
pub(crate) fn url_in_chart(path: &Path, bytes: &[u8]) -> Option<String> {
    let bmson = path
        .extension()
        .is_some_and(|x| x.eq_ignore_ascii_case("bmson"));
    if bmson {
        let v: serde_json::Value = serde_json::from_slice(bytes).ok()?;
        let url = v.get("info").and_then(|i| i.get("url")).or(v.get("url"))?;
        return web_link(url.as_str()?);
    }
    let text = vibebms_core::encoding::decode_text(bytes);
    text.lines().find_map(|l| {
        let l = l.trim_start();
        let head = l.get(..4)?;
        let rest = &l[4..];
        (head.eq_ignore_ascii_case("#URL") && rest.starts_with(char::is_whitespace))
            .then(|| web_link(rest))
            .flatten()
    })
}

/// A difficulty table's link for `s`: its chart's page (差分), else its
/// song's.
fn table_link(ctx: &Ctx, s: &Song) -> Option<String> {
    let same = |e: &bmseeker::table::TableEntry| {
        (!s.md5.is_empty() && e.md5.eq_ignore_ascii_case(&s.md5))
            || (!s.sha256.is_empty() && e.sha256.eq_ignore_ascii_case(&s.sha256))
    };
    let e = (ctx.tables.list.iter())
        .flat_map(|t| t.entries.iter())
        .find(|e| same(e))?;
    web_link(&e.url_diff).or_else(|| web_link(&e.url))
}

impl SelectScene {
    /// F3: the chart's folder in the system's file manager, its file
    /// selected where the system can (a missing chart: nothing).
    pub(super) fn reveal_chart(&mut self, ctx: &mut Ctx) {
        let Some((_, s)) = self.selected_song(ctx) else {
            ctx.toast("Choose a chart to show its folder");
            return;
        };
        let path = Path::new(&s.path);
        if !open::reveal(path) {
            ctx.toast("No file manager here");
        }
    }

    /// U: the selected chart's web page in the browser: its `#URL`, else
    /// the link a difficulty table has for it (a missing chart's too).
    pub(super) fn open_chart_url(&mut self, ctx: &mut Ctx) {
        let url = match self.bars.get(self.cursor) {
            Some(Bar::Song(i)) => ctx.lib.songs.get(*i).and_then(|s| {
                let p = Path::new(&s.path);
                vfs::read(p)
                    .ok()
                    .and_then(|b| url_in_chart(p, &b))
                    .or_else(|| table_link(ctx, s))
            }),
            Some(Bar::Missing(m)) => web_link(&m.url_diff).or_else(|| web_link(&m.url)),
            _ => None,
        };
        match url {
            Some(u) => {
                ctx.toast(format!("Opening {u}"));
                crate::platform::open_url(&u);
            }
            None => ctx.toast("No web page for this chart (#URL, or a table's link)"),
        }
    }

    /// B: SAME FOLDER, the charts in the selected chart's folder, the
    /// cursor on it.
    pub(super) fn same_folder(&mut self, ctx: &mut Ctx) {
        let Some((i, s)) = self.selected_song(ctx) else {
            return;
        };
        let dir = song_dir(s);
        if self.node == Node::SameFolder(dir.clone()) {
            return;
        }
        let path = s.path.clone();
        self.enter(ctx, Node::SameFolder(dir));
        let at = self.bars.iter().position(|b| match b {
            Bar::Song(k) => *k == i || ctx.lib.songs.get(*k).is_some_and(|s| s.path == path),
            _ => false,
        });
        if let Some(c) = at {
            self.cursor = c;
            self.scroll = c as f32;
            self.moved(ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SAME FOLDER lists the charts of the chosen chart's folder, the
    /// cursor on that chart.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn same_folder_lists_the_folder() {
        use std::sync::Arc;
        let root = std::env::temp_dir().join(format!("vibebms-same-{}", std::process::id()));
        let mut ctx = Ctx::for_test(root.clone());
        let song = |path: &str, title: &str| Song {
            path: path.into(),
            folder: "/bms".into(),
            sha256: title.into(),
            title: title.into(),
            ..Default::default()
        };
        ctx.lib.songs = Arc::new(vec![
            song("/bms/x/a.bms", "a"),
            song("/bms/y/b.bms", "b"),
            song("/bms/x/c.bms", "c"),
        ]);
        let mut s = SelectScene::new(&mut ctx);
        s.enter(&mut ctx, Node::All);
        s.cursor = (s.bars.iter())
            .position(|b| matches!(b, Bar::Song(2)))
            .unwrap();
        s.same_folder(&mut ctx);
        assert_eq!(s.node, Node::SameFolder("/bms/x".into()));
        let songs: Vec<usize> = (s.bars.iter())
            .filter_map(|b| match b {
                Bar::Song(i) => Some(*i),
                _ => None,
            })
            .collect();
        assert_eq!(songs, [0, 2]);
        assert!(matches!(s.bars[s.cursor], Bar::Song(2)));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn chart_urls() {
        let bms = Path::new("a.bme");
        let body = b"#TITLE x\r\n#url  https://example.com/song?id=1 \r\n#WAV01 a.wav\r\n";
        assert_eq!(
            url_in_chart(bms, body).as_deref(),
            Some("https://example.com/song?id=1")
        );
        // other headers starting with URL, and links that are no web page
        assert_eq!(url_in_chart(bms, b"#URLX http://a.b/c\n"), None);
        assert_eq!(url_in_chart(bms, b"#URL file:///etc/passwd\n"), None);
        assert_eq!(url_in_chart(bms, b"#URL mail me\n"), None);
        let bmson = Path::new("a.bmson");
        let j = br#"{"version":"1.0.0","info":{"title":"x","url":"http://e.org/x"}}"#;
        assert_eq!(url_in_chart(bmson, j).as_deref(), Some("http://e.org/x"));
        assert_eq!(url_in_chart(bmson, br#"{"info":{}}"#), None);
    }
}
