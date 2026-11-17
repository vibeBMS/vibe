//! Deciding on a bar: into a folder, to the play of a chart or a course,
//! or getting a chart that is not in the library.

use super::tree::{event, table, Bar, Missing, Node};
use super::SelectScene;
use crate::app::{Ctx, Next};
use crate::config::Versus;
use crate::replay::{self, Slot};
use crate::scene::play::{self, CourseRun, PlayRequest, PlayScene};

impl SelectScene {
    pub(super) fn decide(&mut self, ctx: &mut Ctx, autoplay: bool) -> Next {
        let autoplay = autoplay || ctx.config.play.autoplay;
        match self.bars.get(self.cursor).cloned() {
            Some(Bar::Folder {
                node: Node::Close(_),
                ..
            }) => {
                self.back(ctx);
                Next::Stay
            }
            Some(Bar::Folder { node, .. }) => {
                self.enter(ctx, node);
                Next::Stay
            }
            Some(Bar::Song(i)) => {
                let song = ctx.lib.songs[i].clone();
                // G-BATTLE: the opponent's replay, if it can be one
                let slot = match ctx.config.play.versus {
                    Versus::GhostBest => Some(Slot::Best),
                    Versus::GhostLast => Some(Slot::Last),
                    _ => None,
                };
                let vs = match slot.map(|s| play::ghost_for(ctx, &song, s).map(|r| (r, s))) {
                    Some(Ok((r, s))) => Some(play::Versus::Ghost(r, s)),
                    Some(Err(e)) => {
                        ctx.toast(e);
                        return Next::Stay;
                    }
                    None if ctx.config.play.versus == Versus::Local => {
                        if let Some(why) = play::vs_refusal(ctx, &song) {
                            ctx.toast(why);
                            return Next::Stay;
                        }
                        Some(play::Versus::Local)
                    }
                    None => None,
                };
                self.launch(
                    ctx,
                    PlayRequest {
                        song,
                        autoplay,
                        course: None,
                        replay: None,
                        vs,
                    },
                )
            }
            Some(Bar::Missing(m)) => {
                self.fetch(ctx, m);
                Next::Stay
            }
            Some(Bar::Course { table: url, course }) => {
                let Some(c) = table(ctx, &url)
                    .and_then(|t| t.header.courses.get(course))
                    .cloned()
                else {
                    return Next::Stay;
                };
                let mut songs = Vec::new();
                for k in 0..c.md5.len().max(c.sha256.len()) {
                    let md5 = c.md5.get(k).map_or("", String::as_str);
                    let sha = c.sha256.get(k).map_or("", String::as_str);
                    let Some(i) = self.find(&md5.to_lowercase(), &sha.to_lowercase()) else {
                        ctx.toast(format!(
                            "Course {}: chart #{} is not installed",
                            c.name,
                            k + 1
                        ));
                        return Next::Stay;
                    };
                    songs.push(ctx.lib.songs[i].clone());
                }
                if songs.is_empty() {
                    return Next::Stay;
                }
                let run = CourseRun::new(c.name.clone(), songs, c.constraint.clone());
                let song = run.songs[0].clone();
                self.launch(
                    ctx,
                    PlayRequest {
                        song,
                        autoplay,
                        course: Some(run),
                        replay: None,
                        vs: None,
                    },
                )
            }
            None => Next::Stay,
        }
    }

    /// To the play (through the decide screen).
    fn launch(&mut self, ctx: &mut Ctx, req: PlayRequest) -> Next {
        self.stop_sounds(ctx);
        Next::Push(crate::scene::decide::start(ctx, req))
    }

    /// Watch the chosen chart's replay: its best play's, else its latest.
    pub(super) fn watch_replay(&mut self, ctx: &mut Ctx) -> Next {
        let Some((_, song)) = self.selected_song(ctx) else {
            return Next::Stay;
        };
        let song = song.clone();
        let ln = song.score_ln(ctx.config.play.ln_mode) as i32;
        let fixed = song.fixed_ln();
        let load = |slot| replay::load_filed(&ctx.paths.root, &song.sha256, ln, fixed, slot);
        let Some(r) = load(Slot::Best).or_else(|| load(Slot::Last)) else {
            ctx.toast(format!("No replay of {}", song.title));
            return Next::Stay;
        };
        self.stop_sounds(ctx);
        let req = PlayRequest {
            song,
            autoplay: false,
            course: None,
            replay: Some(std::sync::Arc::new(r)),
            vs: None,
        };
        Next::Push(Box::new(PlayScene::new(ctx, req)))
    }

    /// A chart not in the library: downloaded (from a mirrored event, or
    /// its links), else its download pages opened. Chosen again while it
    /// downloads: cancelled.
    fn fetch(&mut self, ctx: &mut Ctx, m: Missing) {
        let Missing {
            title,
            artist,
            md5,
            org_md5,
            more,
            addons,
            url,
            url_diff,
            archive,
            ..
        } = m;
        if let Some((event_url, num)) = archive {
            let root = ctx.paths.root.clone();
            let got = event(ctx, &event_url).ok_or_else(|| "no such event".to_string());
            let msg =
                match got.and_then(|e| ctx.net.archives.download(&root, &ctx.net.hosts, e, num)) {
                    Ok(m) => m,
                    Err(why) => format!("{title}: {why}"),
                };
            ctx.toast(msg);
            return;
        }
        if ctx.net.song_fetch.cancel(&title, &artist) {
            ctx.toast(format!("{title}: cancelled"));
            return;
        }
        if ctx.net.song_fetch.is_busy(&title, &artist) {
            ctx.toast(format!("{title}: being added to the library"));
            return;
        }
        let root = ctx.paths.root.clone();
        // its own links, then the others known for the chart
        let e = bmseeker::table::TableEntry {
            md5: md5.clone(),
            url: url.clone(),
            url_diff: url_diff.clone(),
            ..Default::default()
        };
        let mut song = ctx.tables.links.known.song_links(&e);
        for u in more {
            if !song.contains(&u) {
                song.push(u);
            }
        }
        // its separate chart's links; add-ons each after
        let entry = bmseeker::install::Entry {
            md5: md5.clone(),
            want: crate::hosts::Want {
                title: title.clone(),
                artist: artist.clone(),
            },
            org_md5,
            song,
            chart: ctx.tables.links.known.chart_links(&e),
            ..Default::default()
        };
        // the song is in the library (another of its charts): what to say
        // when nothing can be tried
        let have_song = !md5.is_empty()
            && ctx.lib.songs.iter().any(|s| {
                crate::placement::same_name(&s.title, &title)
                    && crate::placement::same_artist(&s.artist, &artist)
            });
        if ctx
            .net
            .song_fetch
            .start(&root, &ctx.lib.songs, &ctx.net.hosts, entry, addons)
        {
            ctx.toast(format!("Downloading {title}… (choose it again to cancel)"));
            return;
        }
        if have_song {
            ctx.toast(format!(
                "{title}: the song is in the library, but this chart has no download link"
            ));
            return;
        }
        // the song's page, then the separate chart's (the chart goes into
        // the song's folder)
        let pages: Vec<&str> = [url.as_str(), url_diff.as_str()]
            .into_iter()
            .filter(|u| u.starts_with("http://") || u.starts_with("https://"))
            .collect();
        if crate::app::IN_APP_BROWSER && !pages.is_empty() {
            ctx.toast(format!("Download {title} in the browser window"));
            ctx.net.browse = Some(pages.iter().map(|u| u.to_string()).collect());
            return;
        }
        let opened = pages
            .iter()
            .filter(|u| crate::platform::open_url(u))
            .count();
        ctx.toast(match (pages.len(), opened) {
            (0, _) => format!("Not installed: {title} (no download link)"),
            (_, 0) => format!("Not installed: {title} — {}", pages.join("  ")),
            (1, _) => format!("Opened the download page of {title}"),
            _ => format!(
                "Opened the download pages of {title}: put the chart into the song's folder"
            ),
        });
    }
}
