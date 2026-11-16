//! The last hosts asked: those whose links only a browser gets anything
//! from (OneDrive's pages are built by script; uploaders want a click, a
//! password or an account), and any other server, which is taken to send
//! the file itself.

use super::{Host, Link, Resolved, Target, Want};
use crate::config::Config;
use crate::net::{BoxTask, Ready};

/// Hosts whose links always need a browser.
const PAGES: &[&str] = &[
    "1drv.ms",
    "onedrive",
    "sharepoint.com",
    "baidu",
    "lanzou",
    "lanzn",
    "gigafile",
    "box.com",
];

pub struct BrowserOnly;

impl Host for BrowserOnly {
    fn name(&self) -> &'static str {
        "browser"
    }

    fn accepts(&self, link: &Link) -> bool {
        PAGES.iter().any(|p| link.host.contains(p))
    }

    fn open(&self, _: &Link, _: Option<&Want>, _: &Config) -> BoxTask<Resolved> {
        Ready::boxed(Resolved::Page)
    }
}

pub struct Direct;

impl Host for Direct {
    fn name(&self) -> &'static str {
        "direct"
    }

    fn accepts(&self, _: &Link) -> bool {
        true
    }

    fn open(&self, link: &Link, _: Option<&Want>, _: &Config) -> BoxTask<Resolved> {
        Ready::boxed(Resolved::File(Target::url(link.url.clone())))
    }
}
