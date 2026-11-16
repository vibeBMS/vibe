//! Dropbox: files and folders download with `dl=1` (a folder as a zip).

use super::{Host, Link, Resolved, Target, Want};
use crate::config::Config;
use crate::net::{BoxTask, Ready};

pub struct Dropbox;

impl Host for Dropbox {
    fn name(&self) -> &'static str {
        "dropbox"
    }

    fn accepts(&self, link: &Link) -> bool {
        link.on("dropbox.com")
    }

    fn open(&self, link: &Link, _: Option<&Want>, _: &Config) -> BoxTask<Resolved> {
        Ready::boxed(Resolved::File(Target::url(dl1(&link.url))))
    }
}

fn dl1(u: &str) -> String {
    let base = u.replace("dl=0", "dl=1");
    if base.contains("dl=1") {
        base
    } else if base.contains('?') {
        format!("{base}&dl=1")
    } else {
        format!("{base}?dl=1")
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn dl1() {
        assert_eq!(
            super::dl1("https://www.dropbox.com/scl/fo/a/b?rlkey=k&dl=0"),
            "https://www.dropbox.com/scl/fo/a/b?rlkey=k&dl=1"
        );
        assert_eq!(
            super::dl1("https://dropbox.com/s/a/b.zip"),
            "https://dropbox.com/s/a/b.zip?dl=1"
        );
    }
}
