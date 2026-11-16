//! MediaFire: a file's page has the download button's link (a folder's
//! needs a browser).

use super::{between, get_then, unescape, Host, Link, Resolved, Target, Want};
use crate::config::Config;
use crate::net::{BoxTask, Ready};

pub struct MediaFire;

impl Host for MediaFire {
    fn name(&self) -> &'static str {
        "mediafire"
    }

    fn accepts(&self, link: &Link) -> bool {
        link.on("mediafire.com")
    }

    fn open(&self, link: &Link, _: Option<&Want>, _: &Config) -> BoxTask<Resolved> {
        if link.lower.contains("/folder/") {
            return Ready::boxed(Resolved::Page);
        }
        get_then(link.url.clone(), |body| {
            let page = String::from_utf8_lossy(&body);
            // http or https: older pages link the file over http
            let file = page
                .match_indices("href=\"")
                .filter_map(|(at, _)| between(&page[at..], "href=\"", "\""))
                .find(|l| {
                    let l = l.to_ascii_lowercase();
                    (l.starts_with("https://download") || l.starts_with("http://download"))
                        && l.contains("mediafire.com/")
                });
            match file {
                Some(l) => Resolved::File(Target::url(unescape(l))),
                None => Resolved::Page,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::hosts::tests::resolve;

    #[test]
    fn download_buttons() {
        let link = "https://www.mediafire.com/file/x/a.rar/file";
        let page = br#"<a class="input" href="https://download1.mediafire.com/x/a.rar">"#;
        assert_eq!(
            resolve(link, &[(link, page)]).unwrap().url,
            "https://download1.mediafire.com/x/a.rar"
        );
        // an older page: the file over http, after other links
        let page = br#"<a href="http://www.mediafire.com/help"></a><a class="input popsok" href="http://download1336.mediafire.com/k/x/a.zip" id="downloadButton">"#;
        assert_eq!(
            resolve(link, &[(link, page)]).unwrap().url,
            "http://download1336.mediafire.com/k/x/a.zip"
        );
    }
}
