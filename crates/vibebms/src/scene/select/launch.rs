//! Deciding on a bar: into a folder, to the play of a chart or a course,
//! or getting a chart that is not in the library.

use super::tree::{event, table, Bar, BulkBar, CourseBar, Missing, Node};
use super::SelectScene;
use crate::config::Versus;
use crate::ctx::Ctx;
use crate::library::replay;
use crate::scene::play::{self, CourseRun, PlayRequest, PlayScene};
use crate::scene::practice::PracticeScene;
use crate::scene::Next;
use vibebms_core::course::{CourseDef, CourseKind, MAX_STAGES};
use vibebms_core::rng::JavaRandom;

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
                let slot = ctx.config.play.versus.ghost();
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
                        practice: None,
                    },
                )
            }
            Some(Bar::Random) => {
                let pick = super::tree::random_candidates(ctx, &self.bars);
                if pick.is_empty() {
                    let kind = ctx.config.select.random.label();
                    ctx.toast(tr!("RANDOM SELECT: no chart here is {kind}", kind = kind));
                    return Next::Stay;
                }
                let seed = crate::platform::unix_ms();
                let k = JavaRandom::new(seed).next_int_bounded(pick.len() as i32);
                self.cursor = pick[k as usize];
                self.moved(ctx);
                self.decide(ctx, autoplay)
            }
            Some(Bar::Missing(m)) => {
                self.fetch(ctx, m);
                Next::Stay
            }
            Some(Bar::Bulk(b)) => {
                self.bulk(ctx, b);
                Next::Stay
            }
            Some(Bar::Course(CourseBar::RandomMix(kind))) => {
                let seed = crate::platform::unix_ms();
                let mix = ctx.config.random_mix.clone();
                let songs = ctx.lib.songs.clone();
                match ctx
                    .lib
                    .courses
                    .random_mix(kind, &mix, &songs, &self.charts, mix.mode, seed)
                {
                    Ok(id) => {
                        ctx.lib.version += 1;
                        self.rebuild(ctx);
                        if let Some(i) = self
                            .bars
                            .iter()
                            .position(|b| matches!(b, Bar::Course(CourseBar::Mine(x)) if *x == id))
                        {
                            self.cursor = i;
                        }
                        ctx.toast(tr!(
                            "{name}: a new course",
                            name = crate::library::courses::RANDOM_MIX
                        ));
                    }
                    Err(e) => ctx.toast(e),
                }
                Next::Stay
            }
            Some(Bar::Course(c)) => match self.course_run(ctx, &c) {
                Ok(run) => {
                    let song = run.songs[0].clone();
                    self.launch(
                        ctx,
                        PlayRequest {
                            song,
                            autoplay,
                            course: Some(run),
                            replay: None,
                            vs: None,
                            practice: None,
                        },
                    )
                }
                Err(e) => {
                    ctx.toast(e);
                    Next::Stay
                }
            },
            None => Next::Stay,
        }
    }

    /// To the play (through the decide screen).
    fn launch(&mut self, ctx: &mut Ctx, req: PlayRequest) -> Next {
        self.stop_sounds(ctx);
        Next::Push(crate::scene::decide::start(ctx, req))
    }

    /// The chosen chart as the next stage of the course being edited (the
    /// settings' COURSES).
    pub(super) fn add_to_course(&mut self, ctx: &mut Ctx) {
        let Some((_, song)) = self.selected_song(ctx) else {
            return;
        };
        let stage = crate::library::courses::Stage::of(song);
        let st = &mut ctx.lib.courses;
        let Some(c) = st.editing.and_then(|id| st.get_mut(id)) else {
            ctx.toast(tr!("No course open in the editor (settings, COURSES)"));
            return;
        };
        let msg = if c.stages.len() >= vibebms_core::course::MAX_STAGES {
            tr!(
                "{title} has its {n} stages",
                title = c.title,
                n = c.stages.len()
            )
        } else {
            c.stages.push(stage);
            let msg = tr!(
                "{title}: stage {n} added",
                title = c.title,
                n = c.stages.len()
            );
            st.save();
            ctx.lib.version += 1;
            msg
        };
        ctx.toast(msg);
    }

    /// Watch the chosen chart's replay, the one REPLAY picks (the best
    /// play's, else the latest; MY BEST; LAST; a saved slot), at the
    /// watch speed; a course's (`watch_course`).
    pub(super) fn watch_replay(&mut self, ctx: &mut Ctx) -> Next {
        if let Some(Bar::Course(c)) = self.bars.get(self.cursor).cloned() {
            return self.watch_course(ctx, &c);
        }
        let Some((_, song)) = self.selected_song(ctx) else {
            return Next::Stay;
        };
        let song = song.clone();
        let ln = song.score_ln(ctx.config.play.ln_pref()) as i32;
        let fixed = song.fixed_ln() && !ctx.config.play.ln_force;
        let rule = ctx.config.play.judge_preset;
        let pick = ctx.config.select.replay;
        let load =
            |slot| replay::load_filed(&ctx.paths.player, &song.sha256, ln, rule, fixed, slot);
        let Some(r) = pick.slots().into_iter().find_map(load) else {
            ctx.toast(tr!(
                "No {pick} replay of {title}",
                pick = pick_name(pick),
                title = song.title
            ));
            return Next::Stay;
        };
        self.stop_sounds(ctx);
        let req = PlayRequest {
            song,
            autoplay: false,
            course: None,
            replay: Some(std::sync::Arc::new(r)),
            vs: None,
            practice: None,
        };
        Next::Push(Box::new(PlayScene::new(ctx, req)))
    }

    /// Watch a course's replay in the slot REPLAY picks: a NONSTOP
    /// course's play, an expert or class course's stages one after
    /// another (each stage's own replay of that slot, as far as there is
    /// one).
    fn watch_course(&mut self, ctx: &mut Ctx, c: &CourseBar) -> Next {
        let mut run = match self.course_run(ctx, c) {
            Ok(r) => r,
            Err(e) => {
                ctx.toast(e);
                return Next::Stay;
            }
        };
        let pick = ctx.config.select.replay;
        let found = pick.slots().into_iter().find_map(|slot| {
            let r = play::course_replay(ctx, &run, 0, slot)?;
            Some((slot, r))
        });
        let Some((slot, r)) = found else {
            ctx.toast(tr!(
                "No {pick} replay of {title}",
                pick = pick_name(pick),
                title = run.name
            ));
            return Next::Stay;
        };
        run.watch = Some(slot);
        let song = run.songs[0].clone();
        self.stop_sounds(ctx);
        let req = PlayRequest {
            song,
            autoplay: false,
            course: Some(run),
            replay: Some(std::sync::Arc::new(r)),
            vs: None,
            practice: None,
        };
        Next::Push(Box::new(PlayScene::new(ctx, req)))
    }

    /// Open the chosen chart's page on the IR the rankings come from, in
    /// the system's browser (the web: a new tab).
    pub(super) fn open_ir_page(&mut self, ctx: &mut Ctx) {
        let Some((_, song)) = self.selected_song(ctx) else {
            ctx.toast(tr!("IR page: choose a chart").to_string());
            return;
        };
        let chart = vibebms_ir::provider::ChartId {
            md5: song.md5.clone(),
            sha256: song.sha256.clone(),
            mode: song.mode,
            ln: song.score_ln(ctx.config.play.ln_pref()),
            remote: None,
        };
        let title = song.title.clone();
        match ctx.net.ir.chart_page(&ctx.config.ir, chart) {
            Ok(url) => {
                if crate::platform::open_url(&url) {
                    ctx.toast(tr!(
                        "IR page of {title}: opened in the browser",
                        title = title
                    ));
                } else {
                    ctx.toast(tr!("IR page: cannot open {url}", url = url));
                }
            }
            Err(why) => ctx.toast(tr!("IR page: {why}", why = why)),
        }
    }

    /// The chosen chart's practice screen.
    pub(super) fn practice(&mut self, ctx: &mut Ctx) -> Next {
        let Some((_, song)) = self.selected_song(ctx) else {
            return Next::Stay;
        };
        let song = song.clone();
        self.stop_sounds(ctx);
        Next::Push(Box::new(PracticeScene::new(ctx, song)))
    }

    /// A bulk download bar: the table's or level's missing charts queued;
    /// the queue's failed retried, its waiting cancelled, its finished
    /// cleared; a chart of it cancelled or queued again (one that has only
    /// a page: its page opened, as a chart chosen alone).
    fn bulk(&mut self, ctx: &mut Ctx, b: BulkBar) {
        let net = &mut ctx.net;
        let msg = match b {
            BulkBar::Queue(node, _) => {
                let wanted = self.missing_of(ctx, &node);
                let n = ctx.net.bulk.queue(wanted);
                tr!(
                    "{n} charts queued: {at} at a time, in DOWNLOADS at the root",
                    n = n,
                    at = crate::net::bulk::AT_ONCE
                )
            }
            BulkBar::RetryFailed => {
                tr!("{n} charts queued again", n = net.bulk.retry_failed())
            }
            BulkBar::CancelAll => {
                tr!(
                    "{n} downloads cancelled",
                    n = net.bulk.cancel_all(&mut net.song_fetch)
                )
            }
            BulkBar::ClearFinished => {
                net.bulk.clear_finished();
                tr!("Finished downloads cleared").into()
            }
            BulkBar::Item(k) => {
                let Some(it) = net.bulk.items.get(k).cloned() else {
                    return;
                };
                if let crate::net::bulk::State::Page(_) = it.state {
                    let w = it.want;
                    self.fetch(
                        ctx,
                        Missing {
                            title: w.title,
                            artist: w.artist,
                            md5: w.md5,
                            org_md5: w.org_md5,
                            level: w.level,
                            url: w.url,
                            url_diff: w.url_diff,
                            ..Missing::default()
                        },
                    );
                    return;
                }
                if !net.bulk.toggle(k, &mut net.song_fetch) {
                    return;
                }
                match &net.bulk.items[k].state {
                    crate::net::bulk::State::Queued => {
                        tr!("{title}: queued again", title = it.want.title)
                    }
                    _ => tr!("{title}: cancelled", title = it.want.title),
                }
            }
        };
        ctx.toast(msg);
        ctx.lib.version += 1;
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
            let got = event(ctx, &event_url).ok_or_else(|| tr!("no such event").to_string());
            let msg =
                match got.and_then(|e| ctx.net.archives.download(&root, &ctx.net.hosts, e, num)) {
                    Ok(m) => m,
                    Err(why) => format!("{title}: {why}"),
                };
            ctx.toast(msg);
            return;
        }
        if ctx.net.song_fetch.cancel(&title, &artist) {
            ctx.toast(tr!("{title}: cancelled", title = title));
            return;
        }
        if ctx.net.song_fetch.is_busy(&title, &artist) {
            ctx.toast(tr!("{title}: being added to the library", title = title));
            return;
        }
        let root = ctx.paths.root.clone();
        // its own links, then the others known for the chart; its separate
        // chart's links; add-ons each after
        let want = crate::net::songfetch::Wanted {
            title: title.clone(),
            artist: artist.clone(),
            md5: md5.clone(),
            org_md5,
            url: url.clone(),
            url_diff: url_diff.clone(),
            ..Default::default()
        };
        let entry = crate::net::songfetch::entry(&ctx.tables.links, &want, more);
        // the song is in the library (a chart of its title and artist, or
        // one the table names as the song's own, as the download decides):
        // what to say when nothing can be tried
        let have_song = !md5.is_empty() && {
            let song = bmseeker::placement::SongKey::of(&title, &artist);
            (want.org_md5.iter()).any(|m| self.charts.by_md5(&ctx.lib.songs, m, "", "").is_some())
                || self.song_keys(ctx).iter().any(|k| k.same_song(&song))
        };
        if ctx
            .net
            .song_fetch
            .start(&root, &ctx.lib.songs, &ctx.net.hosts, entry, addons, false)
        {
            ctx.toast(tr!(
                "Downloading {title}… (choose it again to cancel)",
                title = title
            ));
            return;
        }
        if have_song {
            ctx.toast(tr!(
                "{title}: the song is in the library, but this chart has no download link",
                title = title
            ));
            return;
        }
        // the song's page, then the separate chart's (the chart goes into
        // the song's folder)
        let pages: Vec<&str> = [url.as_str(), url_diff.as_str()]
            .into_iter()
            .filter(|u| u.starts_with("http://") || u.starts_with("https://"))
            .collect();
        if crate::ctx::IN_APP_BROWSER && !pages.is_empty() {
            ctx.toast(tr!("Download {title} in the browser window", title = title));
            ctx.net
                .browse(pages.iter().map(|u| u.to_string()).collect());
            return;
        }
        let opened = pages
            .iter()
            .filter(|u| crate::platform::open_url(u))
            .count();
        ctx.toast(match (pages.len(), opened) {
            (0, _) => tr!("Not installed: {title} (no download link)", title = title),
            (_, 0) => tr!(
                "Not installed: {title} — {pages}",
                title = title,
                pages = pages.join("  ")
            ),
            (1, _) => tr!("Opened the download page of {title}", title = title),
            _ => tr!(
                "Opened the download pages of {title}: put the chart into the song's folder",
                title = title
            ),
        });
    }
}

/// What a course bar shows that only the library, the tables and the
/// player's courses change: found once per library version
/// (`SelectScene::course_facts`), not per frame.
pub(crate) struct CourseFacts {
    pub kind: CourseKind,
    /// Its stages (RANDOM MIX: none).
    pub stages: Option<usize>,
    pub title: String,
    /// Its identity (`CourseDef::id`): its record (a table course's, when
    /// its charts are installed).
    pub id: Option<String>,
    /// Its charts (library indices), when all are installed.
    pub songs: Vec<usize>,
    /// A table course's trophies: the miss rate and score rate each asks.
    pub trophies: Vec<Trophy>,
}

/// A class course trophy's limits (%).
pub(crate) type Trophy = (Option<f64>, Option<f64>);

/// The limits of a table course's trophies, bronze first.
pub(crate) fn trophy_rates(trophies: &[bmseeker::table::CourseTrophy]) -> Vec<Trophy> {
    (trophies.iter())
        .map(|t| {
            // (bmseeker keeps the rates to itself; they are in its JSON)
            let t = serde_json::to_value(t).unwrap_or_default();
            let rate = |k: &str| t.get(k).and_then(serde_json::Value::as_f64);
            (rate("missrate"), rate("scorerate"))
        })
        .collect()
}

/// A course's record (its identity's, in any LN mode, under the rule
/// mode chosen: the best).
pub(crate) fn course_best<'a>(ctx: &'a Ctx, id: &str) -> Option<&'a crate::library::BestScore> {
    let rule = ctx.config.play.judge_preset;
    (0..3)
        .filter_map(|ln| ctx.best_key(id, ln, rule))
        .max_by_key(|b| b.clear)
}

/// A course of a bar: its name, kind and joins, stages and constraints.
struct Course {
    name: String,
    def: CourseDef,
    /// Its charts' MD5, SHA-256 and title.
    stages: Vec<(String, String, String)>,
    constraints: Vec<String>,
}

fn course_of(ctx: &Ctx, c: &CourseBar) -> Result<Course, String> {
    match c {
        CourseBar::Table(url, i) => {
            let tc = table(ctx, url)
                .and_then(|t| t.header.courses.get(*i))
                .ok_or(tr!("no such course"))?;
            let n = tc.md5.len().max(tc.sha256.len());
            let stages = (0..n)
                .map(|k| {
                    let get =
                        |v: &Vec<String>| v.get(k).map_or(String::new(), |s| s.to_lowercase());
                    (get(&tc.md5), get(&tc.sha256), String::new())
                })
                .collect();
            let def = CourseDef {
                kind: CourseKind::Class,
                ..Default::default()
            };
            Ok(Course {
                name: tc.name.clone(),
                def,
                stages,
                constraints: tc.constraint.clone(),
            })
        }
        CourseBar::Mine(id) => {
            let mc = ctx.lib.courses.get(*id).ok_or(tr!("no such course"))?;
            let stages = (mc.stages.iter())
                .map(|s| (s.md5.clone(), s.sha256.clone(), s.title.clone()))
                .collect();
            Ok(Course {
                name: mc.title.clone(),
                def: mc.def(),
                stages,
                constraints: Vec::new(),
            })
        }
        CourseBar::RandomMix(_) => Err(tr!("not a course").into()),
    }
}

impl SelectScene {
    /// A course's charts (library indices), else why it cannot be played.
    fn stage_charts(&self, ctx: &Ctx, c: &Course) -> Result<Vec<usize>, String> {
        let name = &c.name;
        if c.stages.is_empty() {
            return Err(tr!(
                "Course {name}: no stages yet (add charts in the settings' COURSES)",
                name = name
            ));
        }
        if c.stages.len() > MAX_STAGES {
            return Err(tr!(
                "Course {name}: more than {max} stages",
                name = name,
                max = MAX_STAGES
            ));
        }
        (c.stages.iter().enumerate())
            .map(|(k, (md5, sha, title))| {
                (self.charts.find(&ctx.lib.songs, md5, sha, title, "")).ok_or_else(|| {
                    tr!(
                        "Course {name}: stage {n} is not installed",
                        name = name,
                        n = k + 1
                    )
                })
            })
            .collect()
    }

    /// The course of a bar, ready to play: its stages found in the library
    /// (else why it cannot be played).
    pub(super) fn course_run(&self, ctx: &Ctx, c: &CourseBar) -> Result<CourseRun, String> {
        let mut course = course_of(ctx, c)?;
        let found: Vec<_> = (self.stage_charts(ctx, &course)?.into_iter())
            .map(|i| ctx.lib.songs[i].clone())
            .collect();
        // a table course's identity: its charts' MD5s
        course.def.stages = found.iter().map(|s| s.md5.to_lowercase()).collect();
        Ok(CourseRun::new(
            course.name,
            course.def,
            found,
            course.constraints,
        ))
    }

    /// What course bar `c` shows (`CourseFacts`), worked out when first
    /// asked for in this library version.
    pub(crate) fn course_facts<T>(
        &self,
        ctx: &Ctx,
        c: &CourseBar,
        f: impl FnOnce(&CourseFacts) -> T,
    ) -> T {
        if let Some(facts) = self.courses.borrow().get(c) {
            return f(facts);
        }
        let facts = self.find_course_facts(ctx, c);
        let t = f(&facts);
        self.courses.borrow_mut().insert(c.clone(), facts);
        t
    }

    fn find_course_facts(&self, ctx: &Ctx, c: &CourseBar) -> CourseFacts {
        let table_course =
            |url: &str, k: usize| table(ctx, url).and_then(|t| t.header.courses.get(k));
        let (kind, stages, title) = match c {
            CourseBar::Mine(id) => match ctx.lib.courses.get(*id) {
                Some(c) => (
                    c.kind,
                    Some(c.stages.len()),
                    tr!("{title} ({n} stages)", title = c.title, n = c.stages.len()),
                ),
                None => (CourseKind::Expert, None, String::new()),
            },
            CourseBar::Table(url, k) => {
                let tc = table_course(url, *k);
                (
                    CourseKind::Class,
                    tc.map(|c| c.md5.len().max(c.sha256.len())),
                    tc.map(|c| c.name.clone()).unwrap_or_default(),
                )
            }
            CourseBar::RandomMix(kind) => (*kind, None, crate::library::courses::RANDOM_MIX.into()),
        };
        let course = course_of(ctx, c).ok();
        let songs = (course.as_ref())
            .and_then(|c| self.stage_charts(ctx, c).ok())
            .unwrap_or_default();
        let id = match (c, course) {
            (CourseBar::Mine(_), Some(c)) => Some(c.def.id()),
            (CourseBar::Table(..), Some(mut c)) if !songs.is_empty() => {
                c.def.stages = (songs.iter())
                    .map(|&i| ctx.lib.songs[i].md5.to_lowercase())
                    .collect();
                Some(c.def.id())
            }
            _ => None,
        };
        let trophies = match c {
            CourseBar::Table(url, k) if !songs.is_empty() => table_course(url, *k)
                .map(|c| trophy_rates(&c.trophy))
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        CourseFacts {
            kind,
            stages,
            title,
            id,
            songs,
            trophies,
        }
    }
}

/// REPLAY's choice for people ("BEST, ELSE LAST": any kept).
fn pick_name(p: crate::config::ReplayPick) -> &'static str {
    match p {
        crate::config::ReplayPick::Auto => tr!("kept"),
        p => p.label(),
    }
}
