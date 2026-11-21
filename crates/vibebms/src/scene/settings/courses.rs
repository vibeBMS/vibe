//! COURSES: the player's courses (made, renamed, reordered,
//! deleted; their kind, joins and stages), LR2's imported, and RANDOM MIX's
//! settings. Charts are added from the song select (its C key).

use super::row::{Act, Row, Rows};
use super::{Sub, TextField};
use crate::app::Ctx;
use crate::courses::Course;
use crate::scene::cycle;
use vibebms_core::course::{CourseKind, Join, MAX_STAGES};
use vibebms_core::PlayMode;

/// The course being edited, and the store, once it changed: kept, and the
/// song select told.
fn edit(c: &mut Ctx, f: impl FnOnce(&mut Course, &mut Vec<Join>)) {
    let Some(id) = c.lib.courses.editing else {
        return;
    };
    if let Some(course) = c.lib.courses.get_mut(id) {
        let mut joins = std::mem::take(&mut course.joins);
        f(course, &mut joins);
        course.joins = joins;
    }
    changed(c);
}

fn changed(c: &mut Ctx) {
    c.lib.courses.save();
    c.lib.version += 1;
}

fn new_course(c: &mut Ctx, _: usize) {
    let id = c.lib.courses.add(Course {
        title: tr!("NEW COURSE").into(),
        kind: CourseKind::Expert,
        ..Default::default()
    });
    c.lib.courses.editing = Some(id);
    changed(c);
}

fn delete(c: &mut Ctx, _: usize) {
    if let Some(id) = c.lib.courses.editing.take() {
        c.lib.courses.remove(id);
        c.lib.version += 1;
    }
}

fn shift(c: &mut Ctx, up: usize) {
    if let Some(id) = c.lib.courses.editing {
        c.lib.courses.shift(id, if up == 1 { -1 } else { 1 });
        c.lib.version += 1;
    }
}

fn remove_stage(c: &mut Ctx, k: usize) {
    edit(c, |course, joins| {
        if k < course.stages.len() {
            course.stages.remove(k);
            if k < joins.len() {
                joins.remove(k);
            }
        }
    });
}

fn stage_up(c: &mut Ctx, k: usize) {
    edit(c, |course, _| {
        if k > 0 && k < course.stages.len() {
            course.stages.swap(k - 1, k);
        }
    });
}

fn join(c: &mut Ctx, k: usize) {
    edit(c, |course, joins| {
        if joins.len() < course.stages.len().saturating_sub(1) {
            joins.resize(course.stages.len().saturating_sub(1), Join::default());
        }
        if let Some(j) = joins.get_mut(k) {
            *j = cycle(&Join::ALL, *j, 1);
        }
    });
}

/// Courses from an LR2 install: `path` is its `song.db`, or a folder with
/// `LR2files/Database/song.db` (or `Database/song.db`, or `song.db`) in it.
pub(super) fn import(c: &mut Ctx, path: &str) {
    let p = std::path::Path::new(path.trim());
    let candidates = [
        p.to_path_buf(),
        p.join("LR2files").join("Database").join("song.db"),
        p.join("Database").join("song.db"),
        p.join("song.db"),
    ];
    let Some(bytes) = candidates
        .iter()
        .filter(|f| !f.is_dir())
        .find_map(|f| crate::lr2db::read_file(f).ok())
    else {
        c.toast(tr!("No LR2 song.db at {path}", path = p.display()));
        return;
    };
    match crate::lr2db::courses(&bytes) {
        Ok(list) => {
            let n = list.len();
            let songs = c.lib.songs.clone();
            let added = c.lib.courses.import(list, &songs);
            c.lib.version += 1;
            c.toast(tr!(
                "LR2: {added} of {n} courses imported",
                added = added,
                n = n
            ));
        }
        Err(e) => c.toast(tr!("LR2 song.db: {e}", e = e)),
    }
}

#[cfg(desktop)]
fn browse(c: &mut Ctx, _: usize) {
    if let Some(p) = rfd::FileDialog::new()
        .set_title(tr!("LR2's folder (or its song.db)"))
        .pick_folder()
    {
        import(c, &p.to_string_lossy());
    }
}

fn mix(c: &mut Ctx) -> &mut crate::courses::RandomMix {
    &mut c.config.random_mix
}

pub(super) fn rows(b: &mut Rows) {
    let ctx = b.ctx;
    let st = &ctx.lib.courses;
    b.header(tr!("MY COURSES"));
    b.push(
        Row::new(
            "courses.new",
            tr!("New course"),
            "+",
            Act::Run(new_course, 0),
        )
        .desc(tr!(
            "An empty course; add charts from the song select with C."
        )),
    );
    for kind in [CourseKind::Expert, CourseKind::Nonstop, CourseKind::Class] {
        for c in st.of_kind(kind) {
            let editing = st.editing == Some(c.id);
            b.push(
                Row::sub(
                    "courses.course",
                    c.title.clone(),
                    if editing {
                        tr!("C adds here · edit")
                    } else {
                        tr!("edit")
                    },
                    Sub::Course(c.id),
                )
                .desc(tr!(
                    "{kind} · {n} stages",
                    kind = kind.label(),
                    n = c.stages.len()
                )),
            );
        }
    }
    b.header(tr!("IMPORT"));
    b.push(
        Row::new(
            "courses.import_lr2",
            tr!("Import LR2's courses…"),
            "+",
            Act::Text(TextField::ImportLr2),
        )
        .desc(tr!(
            "From LR2's folder or its song.db: type or paste the path."
        ))
        .keys("lr2 song.db expert grade class"),
    );
    #[cfg(desktop)]
    b.push(
        Row::new(
            "courses.browse_lr2",
            tr!("Browse for LR2's folder…"),
            tr!("browse"),
            Act::Run(browse, 0),
        )
        .desc(tr!("Pick LR2's folder; its courses are imported.")),
    );
    let m = &ctx.config.random_mix;
    let n = |v: i64| {
        if v > 0 {
            v.to_string()
        } else {
            tr!("none").into()
        }
    };
    b.group(
        "courses.random_mix",
        tr!("RANDOM MIX"),
        tr!("The charts RANDOM MIX picks from: levels, BPMs and how NONSTOP stages join"),
    );
    b.extend([
        Row::adjust(
            "random_mix.stages",
            tr!("Stages"),
            m.stages.to_string(),
            |c, d| {
                let s = &mut mix(c).stages;
                *s = (*s as i32 + d).clamp(0, MAX_STAGES as i32) as u32
            },
        )
        .desc(tr!("How many charts a RANDOM MIX course has.")),
        Row::adjust(
            "random_mix.mode",
            tr!("Key mode"),
            m.mode.label(),
            |c, d| {
                let all = [
                    PlayMode::Beat7K,
                    PlayMode::Beat5K,
                    PlayMode::Beat14K,
                    PlayMode::Beat10K,
                    PlayMode::Popn9K,
                ];
                mix(c).mode = cycle(&all, mix(c).mode, d)
            },
        )
        .desc(tr!("The key mode of the charts picked.")),
        Row::adjust(
            "random_mix.min_level",
            tr!("Lowest level"),
            n(m.min_level),
            |c, d| mix(c).min_level = (mix(c).min_level + d as i64).max(0),
        )
        .desc(tr!("No chart below this level (none: no limit).")),
        Row::adjust(
            "random_mix.max_level",
            tr!("Highest level"),
            n(m.max_level),
            |c, d| mix(c).max_level = (mix(c).max_level + d as i64).max(0),
        )
        .desc(tr!("No chart above this level (none: no limit).")),
        Row::adjust(
            "random_mix.min_bpm",
            tr!("Lowest BPM"),
            n(m.min_bpm as i64),
            |c, d| mix(c).min_bpm = (mix(c).min_bpm + 10.0 * d as f64).max(0.0),
        )
        .desc(tr!("No chart slower than this (none: no limit).")),
        Row::adjust(
            "random_mix.max_bpm",
            tr!("Highest BPM"),
            n(m.max_bpm as i64),
            |c, d| mix(c).max_bpm = (mix(c).max_bpm + 10.0 * d as f64).max(0.0),
        )
        .desc(tr!("No chart faster than this (none: no limit).")),
        Row::adjust(
            "random_mix.bpm_range",
            tr!("BPM range"),
            n(m.bpm_range as i64),
            |c, d| mix(c).bpm_range = (mix(c).bpm_range + 5.0 * d as f64).max(0.0),
        )
        .desc(tr!("How far apart the charts' BPMs may be.")),
        Row::adjust(
            "random_mix.optimum_level",
            tr!("Optimum level"),
            n(m.optimum_level),
            |c, d| mix(c).optimum_level = (mix(c).optimum_level + d as i64).max(0),
        )
        .desc(tr!("The level picked most often.")),
        Row::adjust(
            "random_mix.join",
            tr!("NONSTOP join"),
            m.join.label(),
            |c, d| mix(c).join = cycle(&Join::ALL, mix(c).join, d),
        )
        .desc(tr!("How the stages of a NONSTOP course join.")),
    ]);
}

/// COURSES › a course: its title, kind, stages and joins, its place in
/// the list. It is the course C adds charts to in the song select.
pub(super) fn editor(b: &mut Rows, id: u64) {
    let Some(c) = b.ctx.lib.courses.get(id) else {
        return;
    };
    b.header(tr!("COURSE"));
    b.push(
        Row::new(
            "course.title",
            tr!("Title"),
            c.title.clone(),
            Act::Text(TextField::CourseTitle),
        )
        .desc(tr!("Its name in the song select.")),
    );
    b.push(
        Row::adjust("course.kind", tr!("Kind"), c.kind.label(), |c, d| {
            edit(c, |course, _| {
                course.kind = cycle(&CourseKind::ALL, course.kind, d)
            })
        })
        .desc(tr!("EXPERT, NONSTOP or CLASS.")),
    );
    if c.ir {
        b.push(Row::note(
            "course.ir",
            tr!("An LR2 internet-ranking course"),
            "",
        ));
    }
    b.header(tr!(
        "STAGES · {n} OF {max}",
        n = c.stages.len(),
        max = MAX_STAGES
    ));
    let def = c.def();
    for (k, s) in c.stages.iter().enumerate() {
        let name = if s.title.is_empty() { &s.md5 } else { &s.title };
        b.push(
            Row::new(
                "course.stage",
                format!("{}. {name}", k + 1),
                tr!("remove ×"),
                Act::Run(remove_stage, k),
            )
            .desc(tr!("Enter: take it out of the course.")),
        );
        if k > 0 {
            b.push(
                Row::new(
                    "course.stage_up",
                    tr!("Move up"),
                    "↑",
                    Act::Run(stage_up, k),
                )
                .desc(tr!("Swap stages {a} and {b}.", a = k, b = k + 1)),
            );
        }
        if let Some(j) = def.joins.get(k) {
            b.push(
                Row::new(
                    "course.join",
                    tr!("Join {a} → {b}", a = k + 1, b = k + 2),
                    j.label(),
                    Act::Run(join, k),
                )
                .desc(tr!("How the two stages join (NONSTOP).")),
            );
        }
    }
    if c.stages.len() < MAX_STAGES {
        b.push(Row::note(
            "course.add",
            tr!("Add a chart: choose it in the song select, press Insert (or C when no key uses it)"),
            "",
        ));
    }
    b.header(tr!("IN THE LIST"));
    b.extend([
        Row::new(
            "course.shift_up",
            tr!("Move up the list"),
            "↑",
            Act::Run(shift, 1),
        )
        .desc(tr!("Its place among the courses of its kind.")),
        Row::new(
            "course.shift_down",
            tr!("Move down the list"),
            "↓",
            Act::Run(shift, 0),
        )
        .desc(tr!("Its place among the courses of its kind.")),
        Row::new(
            "course.delete",
            tr!("Delete this course"),
            tr!("delete ×"),
            Act::Run(delete, 0),
        )
        .desc(tr!("It cannot be undone.")),
    ]);
}

/// The title of the course being edited.
pub(super) fn title(ctx: &Ctx) -> String {
    ctx.lib
        .courses
        .editing
        .and_then(|id| ctx.lib.courses.get(id))
        .map(|c| c.title.clone())
        .unwrap_or_default()
}

pub(super) fn rename(c: &mut Ctx, s: String) {
    if !s.is_empty() {
        edit(c, |course, _| course.title = s);
    }
}
