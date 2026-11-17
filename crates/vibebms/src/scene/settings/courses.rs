//! The course editor: the player's courses (made, renamed, reordered,
//! deleted; their kind, joins and stages), LR2's imported, and RANDOM MIX's
//! settings. Charts are added from the song select (its C key).

use super::{Act, Row, TextField};
use crate::app::Ctx;
use crate::courses::Course;
use crate::scene::option_panel::cycle;
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
        title: "NEW COURSE".into(),
        kind: CourseKind::Expert,
        ..Default::default()
    });
    c.lib.courses.editing = Some(id);
    changed(c);
}

fn choose(c: &mut Ctx, id: usize) {
    let id = id as u64;
    let st = &mut c.lib.courses;
    st.editing = (st.editing != Some(id)).then_some(id);
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
        .find_map(|f| crate::platform::vfs::read(f).ok())
    else {
        c.toast(format!("No LR2 song.db at {}", p.display()));
        return;
    };
    match crate::lr2db::courses(&bytes) {
        Ok(list) => {
            let n = list.len();
            let songs = c.lib.songs.clone();
            let added = c.lib.courses.import(list, &songs);
            c.lib.version += 1;
            c.toast(format!("LR2: {added} of {n} courses imported"));
        }
        Err(e) => c.toast(format!("LR2 song.db: {e}")),
    }
}

#[cfg(desktop)]
fn browse(c: &mut Ctx, _: usize) {
    if let Some(p) = rfd::FileDialog::new()
        .set_title("LR2's folder (or its song.db)")
        .pick_folder()
    {
        import(c, &p.to_string_lossy());
    }
}

fn mix(c: &mut Ctx) -> &mut crate::courses::RandomMix {
    &mut c.config.random_mix
}

pub(super) fn rows(ctx: &Ctx) -> Vec<Row> {
    let st = &ctx.lib.courses;
    let mut v = vec![
        Row::new("New course", "+", Act::Run(new_course, 0)),
        Row::new(
            "Import LR2's courses (its folder or song.db)…",
            "+",
            Act::Text(TextField::ImportLr2),
        ),
    ];
    #[cfg(desktop)]
    v.push(Row::new(
        "Browse for LR2's folder…",
        "📂",
        Act::Run(browse, 0),
    ));
    for kind in [CourseKind::Expert, CourseKind::Nonstop, CourseKind::Class] {
        for c in st.of_kind(kind) {
            let open = st.editing == Some(c.id);
            v.push(Row::new(
                format!("{} · {} ({} stages)", kind.label(), c.title, c.stages.len()),
                if open { "close ▾" } else { "edit ›" },
                Act::Run(choose, c.id as usize),
            ));
            if open {
                editor(c, &mut v);
            }
        }
    }
    let m = &ctx.config.random_mix;
    let n = |v: i64| if v > 0 { v.to_string() } else { "none".into() };
    v.extend([
        Row::note("RANDOM MIX (expert and NONSTOP course folders)", ""),
        Row::adjust("  Stages", m.stages.to_string(), |c, d| {
            let s = &mut mix(c).stages;
            *s = (*s as i32 + d).clamp(0, MAX_STAGES as i32) as u32
        }),
        Row::adjust("  Key mode", m.mode.label(), |c, d| {
            let all = [
                PlayMode::Beat7K,
                PlayMode::Beat5K,
                PlayMode::Beat14K,
                PlayMode::Beat10K,
                PlayMode::Popn9K,
            ];
            mix(c).mode = cycle(&all, mix(c).mode, d)
        }),
        Row::adjust("  Lowest level", n(m.min_level), |c, d| {
            mix(c).min_level = (mix(c).min_level + d as i64).max(0)
        }),
        Row::adjust("  Highest level", n(m.max_level), |c, d| {
            mix(c).max_level = (mix(c).max_level + d as i64).max(0)
        }),
        Row::adjust("  Lowest BPM", n(m.min_bpm as i64), |c, d| {
            mix(c).min_bpm = (mix(c).min_bpm + 10.0 * d as f64).max(0.0)
        }),
        Row::adjust("  Highest BPM", n(m.max_bpm as i64), |c, d| {
            mix(c).max_bpm = (mix(c).max_bpm + 10.0 * d as f64).max(0.0)
        }),
        Row::adjust("  BPM range", n(m.bpm_range as i64), |c, d| {
            mix(c).bpm_range = (mix(c).bpm_range + 5.0 * d as f64).max(0.0)
        }),
        Row::adjust("  Optimum level", n(m.optimum_level), |c, d| {
            mix(c).optimum_level = (mix(c).optimum_level + d as i64).max(0)
        }),
        Row::adjust("  NONSTOP join", m.join.label(), |c, d| {
            mix(c).join = cycle(&Join::ALL, mix(c).join, d)
        }),
    ]);
    v
}

/// The rows of the course being edited.
fn editor(c: &Course, v: &mut Vec<Row>) {
    v.push(Row::new(
        "    Title",
        c.title.clone(),
        Act::Text(TextField::CourseTitle),
    ));
    v.push(Row::adjust("    Kind", c.kind.label(), |c, d| {
        edit(c, |course, _| {
            course.kind = cycle(&CourseKind::ALL, course.kind, d)
        })
    }));
    if c.ir {
        v.push(Row::note("    (an LR2 internet-ranking course)", ""));
    }
    let def = c.def();
    for (k, s) in c.stages.iter().enumerate() {
        let name = if s.title.is_empty() { &s.md5 } else { &s.title };
        v.push(Row::new(
            format!("    {}. {name}", k + 1),
            "remove ×",
            Act::Run(remove_stage, k),
        ));
        if k > 0 {
            v.push(Row::new("        move up", "↑", Act::Run(stage_up, k)));
        }
        if let Some(j) = def.joins.get(k) {
            v.push(Row::new(
                format!("        join {} → {}", k + 1, k + 2),
                j.label(),
                Act::Run(join, k),
            ));
        }
    }
    if c.stages.len() < MAX_STAGES {
        v.push(Row::note(
            "    Add a chart: choose it in the song select, press Insert (or C when no key uses it)",
            "",
        ));
    }
    v.push(Row::new("    Move up the list", "↑", Act::Run(shift, 1)));
    v.push(Row::new("    Move down the list", "↓", Act::Run(shift, 0)));
    v.push(Row::new(
        "    Delete this course",
        "delete ×",
        Act::Run(delete, 0),
    ));
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
