//! Deciding on a bar: into a folder, to the play of a chart or a course,
//! or getting a chart that is not in the library.

use super::tree::{event, table, Bar, CourseBar, Missing, Node};
use super::SelectScene;
use crate::app::{Ctx, Next};
use crate::config::Versus;
use crate::replay::{self, Slot};
use crate::scene::play::{self, CourseRun, PlayRequest, PlayScene};
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
            Some(Bar::Random) => {
                let pick = super::tree::random_candidates(ctx, &self.bars);
                if pick.is_empty() {
                    let kind = ctx.config.select.random.label();
                    ctx.toast(format!("RANDOM SELECT: no chart here is {kind}"));
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
            Some(Bar::Course(CourseBar::RandomMix(kind))) => {
                let seed = crate::platform::unix_ms();
                let mix = ctx.config.random_mix.clone();
                let songs = ctx.lib.songs.clone();
                match ctx
                    .lib
                    .courses
                    .random_mix(kind, &mix, &songs, mix.mode, seed)
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
                        ctx.toast(format!("{}: a new course", crate::courses::RANDOM_MIX));
                    }
                    Err(e) => ctx.toast(e),
                }
                Next::Stay
            }
            Some(Bar::Course(c)) => match course_run(ctx, &c) {
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
        let stage = crate::courses::Stage::of(song);
        let st = &mut ctx.lib.courses;
        let Some(c) = st.editing.and_then(|id| st.get_mut(id)) else {
            ctx.toast("No course open in the editor (settings, COURSES)");
            return;
        };
        let msg = if c.stages.len() >= vibebms_core::course::MAX_STAGES {
            format!("{} has its {} stages", c.title, c.stages.len())
        } else {
            c.stages.push(stage);
            let msg = format!("{}: stage {} added", c.title, c.stages.len());
            st.save();
            ctx.lib.version += 1;
            msg
        };
        ctx.toast(msg);
    }

    /// Watch the chosen chart's replay: its best play's, else its latest.
    pub(super) fn watch_replay(&mut self, ctx: &mut Ctx) -> Next {
        let Some((_, song)) = self.selected_song(ctx) else {
            return Next::Stay;
        };
        let song = song.clone();
        let ln = song.score_ln(ctx.config.play.ln_mode) as i32;
        let fixed = song.fixed_ln();
        let rule = ctx.config.play.judge_preset;
        let load = |slot| replay::load_filed(&ctx.paths.root, &song.sha256, ln, rule, fixed, slot);
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

/// The title of a course bar.
pub(crate) fn course_title(ctx: &Ctx, c: &CourseBar) -> String {
    match c {
        CourseBar::Table(url, i) => table(ctx, url)
            .and_then(|t| t.header.courses.get(*i))
            .map(|c| c.name.clone())
            .unwrap_or_default(),
        CourseBar::Mine(id) => ctx
            .lib
            .courses
            .get(*id)
            .map(|c| format!("{} ({} stages)", c.title, c.stages.len()))
            .unwrap_or_default(),
        CourseBar::RandomMix(_) => crate::courses::RANDOM_MIX.into(),
    }
}

/// A course's record (its identity's, in any LN mode, under the rule
/// mode chosen: the best).
pub(crate) fn course_best<'a>(ctx: &'a Ctx, id: &str) -> Option<&'a crate::library::BestScore> {
    let rule = ctx.config.play.judge_preset;
    (0..3)
        .filter_map(|ln| ctx.best_key(id, ln, rule))
        .max_by_key(|b| b.clear)
}

/// The lamp of a course bar's course (`None`: not played, or RANDOM MIX).
pub(crate) fn course_lamp(ctx: &Ctx, c: &CourseBar) -> Option<vibebms_core::score::ClearType> {
    let def = match c {
        CourseBar::Mine(id) => ctx.lib.courses.get(*id)?.def(),
        CourseBar::Table(..) => course_run(ctx, c).ok()?.def,
        CourseBar::RandomMix(_) => return None,
    };
    course_best(ctx, &def.id()).map(|b| b.clear)
}

/// The course of a bar, ready to play: its stages found in the library
/// (else why it cannot be played).
pub(crate) fn course_run(ctx: &Ctx, c: &CourseBar) -> Result<CourseRun, String> {
    use crate::courses::find;
    use vibebms_core::course::{CourseDef, CourseKind, MAX_STAGES};
    let songs = &ctx.lib.songs;
    let (name, mut def, stages, constraints): (
        String,
        CourseDef,
        Vec<(String, String)>,
        Vec<String>,
    ) = match c {
        CourseBar::Table(url, i) => {
            let tc = table(ctx, url)
                .and_then(|t| t.header.courses.get(*i))
                .ok_or("no such course")?;
            let n = tc.md5.len().max(tc.sha256.len());
            let stages = (0..n)
                .map(|k| {
                    let get =
                        |v: &Vec<String>| v.get(k).map_or(String::new(), |s| s.to_lowercase());
                    (get(&tc.md5), get(&tc.sha256))
                })
                .collect();
            let def = CourseDef {
                kind: CourseKind::Class,
                ..Default::default()
            };
            (tc.name.clone(), def, stages, tc.constraint.clone())
        }
        CourseBar::Mine(id) => {
            let mc = ctx.lib.courses.get(*id).ok_or("no such course")?;
            let stages = mc
                .stages
                .iter()
                .map(|s| (s.md5.clone(), s.sha256.clone()))
                .collect();
            (mc.title.clone(), mc.def(), stages, Vec::new())
        }
        CourseBar::RandomMix(_) => return Err("not a course".into()),
    };
    if stages.is_empty() {
        return Err(format!(
            "Course {name}: no stages yet (add charts in the settings' COURSES)"
        ));
    }
    if stages.len() > MAX_STAGES {
        return Err(format!("Course {name}: more than {MAX_STAGES} stages"));
    }
    let mut found = Vec::new();
    for (k, (md5, sha)) in stages.iter().enumerate() {
        let Some(s) = find(songs, md5, sha) else {
            return Err(format!("Course {name}: stage {} is not installed", k + 1));
        };
        found.push(s.clone());
    }
    // a table course's identity: its charts' MD5s
    def.stages = found.iter().map(|s| s.md5.to_lowercase()).collect();
    Ok(CourseRun::new(name, def, found, constraints))
}
