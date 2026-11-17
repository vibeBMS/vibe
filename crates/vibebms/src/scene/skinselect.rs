//! The skin select screen drawn by a SKIN_SELECT skin (LR2 / beatoraja
//! skin type 9): choose the skin of each skin type and its custom options,
//! with a preview of the chosen skin. Sound sets are a skin type here
//! (SOUND SET), as in LR2.
//!
//! ↑↓ (the turntable) pick a row (the skin, then its options), ←→ (white /
//! blue keys) change it, PgUp / PgDn (START) the skin type, the wheel
//! scrolls the options, Esc (SELECT, BACK) leaves and saves.
//!
//! Skin values (beatoraja's, which follow LR2's): buttons 170-185 (the
//! skin type, clicked to choose it), 190 (next / previous skin), 220-229
//! (option row `i`), texts 50 / 51 (the skin's name / author), 100-109
//! (option names), 110-119 (their values), slider 7 (where the option
//! list is scrolled), the preview (LR2 image 105, beatoraja `skinpreview`).

use super::settings::Page;
use super::skin_host::{NoHooks, SkinHost, Tap};
use super::skinned;
use super::skinvals::{self, ms_since};
use crate::app::{Ctx, Draw, Next, Scene};
use crate::gfx::text::{Align, TextStyle};
use crate::gfx::{with_alpha, Gfx, Rect};
use crate::input::{nav_from_key, nav_from_pad_rows, AxisStepper, InputEv, Nav, Phase};
use crate::platform::Instant;
use crate::skin::render::{Hooks, LoadedSkin};
use crate::skin::{CustomOffset, CustomOption, SkinState, SkinType, OFFSET_FIELDS};
use crate::sysound::Sound;
use crate::theme;
use crate::ui::{self, ListView};
use winit::keyboard::KeyCode;

/// The skin types offered, in the order PgUp / PgDn walk them.
pub const TYPES: [SkinType; 14] = [
    SkinType::Play7,
    SkinType::Play5,
    SkinType::Play14,
    SkinType::Play10,
    SkinType::Play9,
    SkinType::Battle7,
    SkinType::Battle5,
    SkinType::Select,
    SkinType::Decide,
    SkinType::Result,
    SkinType::CourseResult,
    SkinType::KeyConfig,
    SkinType::SkinSelect,
    SkinType::SoundSet,
];

/// Button of skin type `t` (170 + its number).
pub fn type_button(t: SkinType) -> i32 {
    170 + t.id()
}

/// One row of the option list.
#[derive(Clone, Debug)]
pub enum Item {
    Option(CustomOption),
    /// An offset's field (x, y, w, h, angle, alpha).
    Offset(CustomOffset, usize),
}

impl Item {
    pub fn name(&self) -> String {
        match self {
            Item::Option(o) => o.name.clone(),
            Item::Offset(o, f) => format!("{} - {}", o.name, OFFSET_FIELDS[*f].to_lowercase()),
        }
    }
}

/// The option rows of the skin chosen as `value` (options, files, then
/// offsets), as beatoraja lists them.
pub fn items(ctx: &Ctx, kind: SkinType, value: &str) -> Vec<Item> {
    if kind == SkinType::SoundSet {
        return Vec::new();
    }
    let Some(p) = super::play_skin::configured_path(ctx, value) else {
        return Vec::new();
    };
    let mut v: Vec<Item> = ctx
        .skins
        .custom_options(&p)
        .iter()
        .filter(|o| !o.labels.is_empty())
        .cloned()
        .map(Item::Option)
        .collect();
    for o in ctx.skins.custom_offsets(&p).iter() {
        for f in 0..6 {
            if o.fields[f] {
                v.push(Item::Offset(o.clone(), f));
            }
        }
    }
    v
}

/// What an option row shows.
pub fn item_value(ctx: &Ctx, value: &str, it: &Item) -> String {
    match it {
        Item::Option(o) => {
            let chosen = ctx
                .config
                .skin_options
                .get(value)
                .and_then(|m| m.get(&o.name))
                .map(String::as_str);
            o.labels.get(o.chosen(chosen)).cloned().unwrap_or_default()
        }
        Item::Offset(o, f) => ctx
            .config
            .skin_offsets
            .get(value)
            .and_then(|m| m.get(&o.name))
            .map(|v| v[*f])
            .unwrap_or(0)
            .to_string(),
    }
}

/// The screen to open for choosing skins (on `kind`'s page): the skinned
/// one when a SKIN_SELECT skin is chosen, else the settings' skin rows
/// (the sound set's for SOUND_SET).
pub fn open(ctx: &mut Ctx, kind: Option<SkinType>) -> Box<dyn Scene> {
    if skinned::configured_skin(ctx, SkinType::SkinSelect).is_some() {
        Box::new(SkinSelectScene::new(kind))
    } else {
        let page = if kind == Some(SkinType::SoundSet) {
            Page::Audio
        } else {
            Page::Display
        };
        Box::new(super::settings::SettingsScene::at(ctx, page))
    }
}

pub struct SkinSelectScene {
    host: SkinHost,
    /// The skin type being chosen (`None`: the skin's default category).
    kind: Option<SkinType>,
    /// The chosen row (0: the skin; 1..: option row `sel - 1`) and the
    /// first option row shown.
    list: ListView,
    back_rect: Rect,
    axis: AxisStepper,
    /// The chosen skin drawn small, and what it was loaded for.
    preview: Option<LoadedSkin>,
    preview_for: Option<(SkinType, String, u64)>,
    preview_at: Instant,
    /// Bumped by every option change (the preview loads again).
    changes: u64,
    changed_at: Instant,
}

impl SkinSelectScene {
    pub fn new(kind: Option<SkinType>) -> SkinSelectScene {
        SkinSelectScene {
            host: SkinHost::new(),
            kind,
            list: ListView::new(),
            back_rect: Rect::default(),
            axis: AxisStepper::default(),
            preview: None,
            preview_for: None,
            preview_at: Instant::now(),
            changes: 0,
            changed_at: Instant::now(),
        }
    }

    pub fn kind(&self) -> SkinType {
        self.kind.unwrap_or(SkinType::Play7)
    }

    fn key(&self) -> &'static str {
        self.kind().key().unwrap_or("7KEYS")
    }

    /// The `config.skins` value of the type shown.
    pub fn value(&self, ctx: &Ctx) -> String {
        ctx.config
            .skins
            .get(self.key())
            .cloned()
            .unwrap_or_default()
    }

    fn items(&self, ctx: &Ctx) -> Vec<Item> {
        items(ctx, self.kind(), &self.value(ctx))
    }

    /// The first option row shown.
    fn offset(&self) -> usize {
        self.list.scroll as usize
    }

    /// Option rows the skin has room for.
    fn rows(&self) -> usize {
        let Some(sk) = self.host.skin.as_ref() else {
            return 6;
        };
        match sk.skin.skin_select.map(|s| s.custom_property_count) {
            Some(n) if n > 0 => n,
            _ => sk
                .skin
                .objects
                .iter()
                .filter_map(|o| o.act.map(|a| a.0))
                .filter(|a| (220..230).contains(a))
                .map(|a| (a - 219) as usize)
                .max()
                .unwrap_or(6),
        }
    }

    pub fn set_kind(&mut self, ctx: &mut Ctx, t: SkinType) {
        if self.kind != Some(t) {
            self.kind = Some(t);
            self.list = ListView::new();
            ctx.play_sound(Sound::OptionOpen);
        }
    }

    fn step_kind(&mut self, ctx: &mut Ctx, d: i32) {
        let i = TYPES.iter().position(|t| *t == self.kind()).unwrap_or(0) as i32;
        let n = TYPES.len() as i32;
        self.set_kind(ctx, TYPES[(i + d).rem_euclid(n) as usize]);
    }

    /// The next / previous skin of the type.
    pub fn step_skin(&mut self, ctx: &mut Ctx, d: i32) {
        let kind = self.kind();
        let opts = skinned::skin_choices(ctx, kind);
        let cur = self.value(ctx);
        let i = opts.iter().position(|o| *o == cur).unwrap_or(0) as i32;
        let next = opts[(i + d).rem_euclid(opts.len() as i32) as usize].clone();
        ctx.config.skins.insert(self.key().to_string(), next);
        self.list = ListView::new();
        self.changed(ctx);
    }

    /// Change option row `i` (of all of them) by `d`.
    pub fn step_item(&mut self, ctx: &mut Ctx, i: usize, d: i32) {
        let value = self.value(ctx);
        match self.items(ctx).get(i) {
            Some(Item::Option(o)) => skinned::step_option(ctx, &value, o, d),
            Some(Item::Offset(o, f)) => skinned::step_offset(ctx, &value, &o.name, *f, d),
            None => return,
        }
        self.changed(ctx);
    }

    fn changed(&mut self, ctx: &mut Ctx) {
        self.changes += 1;
        self.changed_at = Instant::now();
        ctx.save_config();
        ctx.play_sound(Sound::OptionChange);
    }

    /// Change the row under the cursor.
    fn change(&mut self, ctx: &mut Ctx, d: i32) {
        match self.list.sel {
            0 => self.step_skin(ctx, d),
            n => self.step_item(ctx, n - 1, d),
        }
    }

    fn step(&mut self, ctx: &Ctx, d: i32) {
        let n = self.items(ctx).len();
        self.list.step(d, n + 1);
        // (row 0, the skin, is above the option rows)
        self.list.follow = self.list.sel > 0;
        self.fit(n);
    }

    fn scroll(&mut self, ctx: &Ctx, d: i32) {
        self.list.wheel(d as f32);
        self.fit(self.items(ctx).len());
    }

    /// The chosen option row in view (when following) and the rows shown
    /// within the `n` there are.
    fn fit(&mut self, n: usize) {
        let i = self.list.sel.saturating_sub(1) as f32;
        self.list.fit(i, i + 1.0, self.rows() as f32, n as f32);
    }

    fn leave(&mut self, ctx: &mut Ctx) -> Next {
        if self.host.leaving.is_none() {
            ctx.save_config();
            self.host.leave(Instant::now());
        }
        Next::Stay
    }

    /// A clicked skin object; `dir` is +1 / -1 by where it was clicked.
    pub fn event(&mut self, ctx: &mut Ctx, act: i32, dir: i32) {
        match act {
            170..=185 => {
                if let Some(t) = TYPES.iter().find(|t| type_button(**t) == act) {
                    self.set_kind(ctx, *t);
                }
            }
            190 => {
                self.list.sel = 0;
                self.step_skin(ctx, dir);
            }
            220..=229 => {
                let i = self.offset() + (act - 220) as usize;
                if i < self.items(ctx).len() {
                    self.list.sel = i + 1;
                    self.step_item(ctx, i, dir);
                }
            }
            _ => {}
        }
    }

    fn nav(&mut self, ctx: &mut Ctx, nav: Nav) -> Next {
        match nav {
            Nav::Up => self.step(ctx, -1),
            Nav::Down => self.step(ctx, 1),
            Nav::Decide => self.change(ctx, 1),
            Nav::Close => self.change(ctx, -1),
            Nav::Option => self.step_kind(ctx, 1),
            Nav::Back | Nav::Settings => return self.leave(ctx),
        }
        Next::Stay
    }

    /// Load the preview of the chosen skin when it changed (a moment after
    /// the last change: stepping through skins stays quick).
    fn update_preview(&mut self, ctx: &Ctx, gfx: &mut Gfx) {
        let kind = self.kind();
        let want = (kind, self.value(ctx), self.changes);
        if self.preview_for.as_ref() == Some(&want) || self.changed_at.elapsed().as_millis() < 250 {
            return;
        }
        self.preview_for = Some(want.clone());
        if let Some(mut p) = self.preview.take() {
            p.free(gfx);
        }
        // beatoraja shows none for these
        if matches!(
            kind,
            SkinType::SkinSelect | SkinType::SoundSet | SkinType::Result | SkinType::CourseResult
        ) {
            return;
        }
        let Some(path) = skinned::choice_path(ctx, kind, &want.1) else {
            return;
        };
        let loaded = crate::skin::load_file(
            &path,
            &ctx.paths.skins,
            &ctx.skin_user_settings(&want.1, &path),
            &|_| None,
            Some(&PreviewState),
            &crate::skin::image_size,
        );
        match loaded {
            Ok(s) => {
                self.preview = Some(LoadedSkin::load(s, gfx));
                self.preview_at = Instant::now();
            }
            Err(e) => log::warn!("skin preview: {e}"),
        }
    }
}

/// What a previewed skin sees: its start, nothing playing.
struct PreviewState;

impl SkinState for PreviewState {
    fn timer(&self, id: i32) -> Option<i64> {
        (id == 0).then_some(0)
    }
    fn option(&self, id: i32) -> bool {
        id < 0
    }
    fn number(&self, id: i32) -> Option<i64> {
        skinvals::clock_number(id)
    }
    fn text(&self, _: i32) -> Option<String> {
        None
    }
    fn rate(&self, _: i32) -> f32 {
        0.0
    }
}

struct SelectState<'a> {
    s: &'a SkinSelectScene,
    ctx: &'a Ctx,
    items: &'a [Item],
}

impl SkinState for SelectState<'_> {
    fn timer(&self, id: i32) -> Option<i64> {
        self.s.host.timer(id)
    }

    fn option(&self, id: i32) -> bool {
        id < 0
    }

    fn number(&self, id: i32) -> Option<i64> {
        skinvals::clock_number(id).or(match id {
            20 => Some(self.ctx.view.fps as i64),
            _ => None,
        })
    }

    fn text(&self, id: i32) -> Option<String> {
        let value = self.s.value(self.ctx);
        Some(match id {
            2 => self.ctx.config.player_name.clone(),
            50 | 51 => {
                let (n, a) = skinned::choice_name(self.ctx, self.s.kind(), &value);
                if id == 50 {
                    n
                } else {
                    a
                }
            }
            100..=109 => self
                .items
                .get(self.s.offset() + (id - 100) as usize)?
                .name(),
            110..=119 => {
                let it = self.items.get(self.s.offset() + (id - 110) as usize)?;
                item_value(self.ctx, &value, it)
            }
            _ => return None,
        })
    }

    fn rate(&self, id: i32) -> f32 {
        match id {
            7 => {
                let max = self.items.len().saturating_sub(self.s.rows());
                if max == 0 {
                    0.0
                } else {
                    self.s.offset() as f32 / max as f32
                }
            }
            _ => 0.0,
        }
    }

    fn button(&self, id: i32) -> Option<i64> {
        match id {
            170..=185 | 386..=388 => Some((type_button(self.s.kind()) == id) as i64),
            _ => skinvals::config_button(self.ctx, id).or_else(|| self.number(id)),
        }
    }
}

struct SelectHooks<'a> {
    preview: Option<&'a LoadedSkin>,
    preview_ms: i64,
    /// The screen of the skin select skin (the clip to restore).
    screen: Rect,
}

impl Hooks for SelectHooks<'_> {
    fn preview(&mut self, d: &mut Draw, rect: Rect, color: [f32; 4]) {
        let Some(p) = self.preview else { return };
        d.b.rect(rect, [0.0, 0.0, 0.0, color[3]]);
        p.draw(d, rect, self.preview_ms, &PreviewState, &mut NoHooks);
        d.b.set_base_clip(Some(self.screen));
    }
}

impl Scene for SkinSelectScene {
    fn update(&mut self, ctx: &mut Ctx, gfx: &mut Gfx, now: Instant) -> Next {
        if self.host.first_update() {
            let skin = {
                let items = self.items(ctx);
                let st = SelectState {
                    s: self,
                    ctx,
                    items: &items,
                };
                skinned::load(ctx, gfx, SkinType::SkinSelect, &|_| None, &st)
            };
            let Some(skin) = skin else {
                return Next::Replace(Box::new(super::settings::SettingsScene::at(
                    ctx,
                    Page::Display,
                )));
            };
            if self.kind.is_none() {
                let c = skin
                    .skin
                    .skin_select
                    .map(|s| s.default_category)
                    .unwrap_or(0);
                self.kind = Some(
                    TYPES
                        .into_iter()
                        .find(|t| t.id() == c)
                        .unwrap_or(SkinType::Play7),
                );
            }
            self.host.skin = Some(skin);
            self.host.created = now;
        }
        self.host.tick(gfx);
        self.update_preview(ctx, gfx);
        if let Some(p) = self.preview.as_mut() {
            p.load_pending(gfx);
        }
        if self.host.faded(now) {
            return Next::Pop;
        }
        Next::Stay
    }

    fn exit(&mut self, ctx: &mut Ctx, gfx: &mut Gfx) {
        ctx.save_config();
        self.host.free(gfx);
        if let Some(mut p) = self.preview.take() {
            p.free(gfx);
        }
    }

    fn realtime(&self) -> bool {
        true
    }

    fn input(&mut self, ctx: &mut Ctx, ev: &InputEv) -> Next {
        if self.host.leaving.is_some() {
            return Next::Stay;
        }
        let ready = self.host.ready(Instant::now());
        match ev {
            InputEv::Key {
                code,
                pressed: true,
                ..
            } => match code {
                KeyCode::ArrowLeft => self.change(ctx, -1),
                KeyCode::ArrowRight => self.change(ctx, 1),
                KeyCode::PageUp | KeyCode::BracketLeft => self.step_kind(ctx, -1),
                KeyCode::PageDown | KeyCode::BracketRight => self.step_kind(ctx, 1),
                c => {
                    if let Some(n) = nav_from_key(*c, &ctx.config.keys) {
                        if ready || matches!(n, Nav::Back | Nav::Settings) {
                            return self.nav(ctx, n);
                        }
                    }
                }
            },
            InputEv::Pad {
                id, pressed: true, ..
            } => {
                if let Some(n) = nav_from_pad_rows(*id, &ctx.config.keys) {
                    return self.nav(ctx, n);
                }
            }
            InputEv::Axis { id, value, .. } => match self.axis.step(*id, *value) {
                0 => {}
                d => self.step(ctx, d),
            },
            InputEv::Wheel { dy } => {
                let n = (-dy).round() as i32;
                if n != 0 {
                    self.scroll(ctx, n);
                }
            }
            InputEv::Pointer {
                phase: Phase::Up,
                x,
                y,
                ..
            } => {
                if self.back_rect.contains(*x, *y) {
                    return self.leave(ctx);
                }
                if let Tap::Event(act, dir) = self.host.tap(*x, *y) {
                    self.event(ctx, act, dir);
                }
            }
            _ => {}
        }
        Next::Stay
    }

    fn draw(&mut self, ctx: &mut Ctx, d: &mut Draw, now: Instant) {
        let view = Rect::new(0.0, 0.0, ctx.view.width, ctx.view.height);
        let items = self.items(ctx);
        let (sw, sh) = self.host.size();
        let mut hooks = SelectHooks {
            preview: self.preview.as_ref(),
            preview_ms: ms_since(self.preview_at, now),
            screen: self.host.xf(view).rect(Rect::new(0.0, 0.0, sw, sh)),
        };
        let st = SelectState {
            s: self,
            ctx,
            items: &items,
        };
        self.host.draw(d, view, now, &st, &mut hooks);
        // the row the keys change, over its button
        let want = match (self.list.sel, self.offset()) {
            (0, _) => Some(190),
            (n, o) => (o..o + 10)
                .contains(&(n - 1))
                .then(|| 220 + (n - 1 - o) as i32),
        };
        if let Some(r) = want.and_then(|a| self.host.rect_of(a)) {
            d.b.rect(r, with_alpha(theme::ACCENT, 0.12));
            let t = 2.0;
            for e in [
                Rect::new(r.x, r.y, r.w, t),
                Rect::new(r.x, r.y + r.h - t, r.w, t),
                Rect::new(r.x, r.y, t, r.h),
                Rect::new(r.x + r.w - t, r.y, t, r.h),
            ] {
                d.b.rect(e, theme::ACCENT);
            }
        }
        let ui = ctx.ui();
        let (w, h) = (ctx.view.width, ctx.view.height);
        // the keys, for a moment after the screen opens
        let shown = self.host.ms(now);
        if shown < 6000 {
            let a = ((6000 - shown) as f32 / 1000.0).min(1.0);
            let r = Rect::new(w * 0.5 - 260.0 * ui, h - 34.0 * ui, 520.0 * ui, 26.0 * ui);
            d.b.rect(r, with_alpha(theme::BACKGROUND, 0.8 * a));
            d.text(
                "↑↓ row  ←→ change  PgUp/PgDn skin type  Esc back",
                w * 0.5,
                r.y + 4.0 * ui,
                TextStyle::new(15.0 * ui, with_alpha(theme::TEXT, a), Align::Center),
            );
        }
        self.back_rect = ui::back_button(d, w, ui);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_buttons() {
        assert_eq!(type_button(SkinType::Play7), 170);
        assert_eq!(type_button(SkinType::KeyConfig), 178);
        assert_eq!(type_button(SkinType::SoundSet), 180);
        assert_eq!(type_button(SkinType::CourseResult), 185);
    }

    fn write(p: &std::path::Path, s: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, s).unwrap();
    }

    #[test]
    fn skin_types_and_preview() {
        let dir = std::env::temp_dir().join(format!("vibebms-sst-{}", std::process::id()));
        let json = r#"{"type": 9, "name": "SS", "w": 1280, "h": 720,
            "skinSelect": {"defaultCategory": 6, "customPropertyCount": 4},
            "skinpreview": {"id": "pv"},
            "destination": [{"id": "pv", "dst": [{"x": 0, "y": 0, "w": 640, "h": 360}]}]}"#;
        write(&dir.join("ss/skinselect.json"), json);
        write(
            &dir.join("kc/keyconfig.json"),
            r#"{"type": 8, "name": "KC", "w": 1280, "h": 720}"#,
        );
        write(
            &dir.join("lr2/ss.lr2skin"),
            "#INFORMATION,9,LSS,me\n#SRC_IMAGE,0,105,0,0,1280,720,1,1,0,0\n\
             #DST_IMAGE,0,0,0,0,640,360,0,255,255,255,255\n",
        );
        let h = |p: &str| crate::skin::header(&dir.join(p)).map(|h| h.0);
        assert_eq!(h("ss/skinselect.json"), Some(SkinType::SkinSelect));
        assert_eq!(h("kc/keyconfig.json"), Some(SkinType::KeyConfig));
        assert_eq!(h("lr2/ss.lr2skin"), Some(SkinType::SkinSelect));
        let size = |_: &std::path::Path| None;
        let load = |p: &str| {
            crate::skin::load_file(
                &dir.join(p),
                &dir,
                &Default::default(),
                &|_| None,
                None,
                &size,
            )
            .unwrap()
        };
        let ss = load("ss/skinselect.json");
        let lr2 = load("lr2/ss.lr2skin");
        std::fs::remove_dir_all(&dir).ok();
        let info = ss.skin_select.unwrap();
        assert_eq!((info.default_category, info.custom_property_count), (6, 4));
        assert!(matches!(
            ss.objects[0].kind,
            crate::skin::Kind::RefImage(crate::skin::REF_SKIN_PREVIEW)
        ));
        assert!(lr2.skin_select.is_some());
        assert!(matches!(
            lr2.objects[0].kind,
            crate::skin::Kind::RefImage(crate::skin::REF_SKIN_PREVIEW)
        ));
    }

    #[test]
    fn choose_skins_and_options() {
        let root = std::env::temp_dir().join(format!("vibebms-ssc-{}", std::process::id()));
        let opt = |name: &str| {
            format!(
                r#"{{"type": 5, "name": "{name}", "w": 1280, "h": 720,
                "property": [{{"name": "Color", "item": [{{"name": "Red", "op": 900}},
                {{"name": "Blue", "op": 901}}]}}],
                "offset": [{{"name": "Bar", "id": 40, "x": true, "y": true}}]}}"#
            )
        };
        write(&root.join("skins/a/select.json"), &opt("A"));
        write(&root.join("skins/b/select.json"), &opt("B"));
        let mut ctx = Ctx::for_test(root.clone());
        let mut s = SkinSelectScene::new(Some(SkinType::Select));
        // the built-in default, then the two skins
        s.step_skin(&mut ctx, 1);
        assert_eq!(s.value(&ctx), "skins/a/select.json");
        s.event(&mut ctx, 190, 1);
        assert_eq!(s.value(&ctx), "skins/b/select.json");
        let items = s.items(&ctx);
        assert_eq!(items.len(), 3); // Color, Bar - x, Bar - y
                                    // option row 1 (button 220): Red -> Blue; row 3 (222): Bar y + 1
        s.event(&mut ctx, 220, 1);
        s.event(&mut ctx, 222, 1);
        s.event(&mut ctx, 222, 1);
        let st = SelectState {
            s: &s,
            ctx: &ctx,
            items: &items,
        };
        assert_eq!(st.text(50).as_deref(), Some("B"));
        assert_eq!(st.text(100).as_deref(), Some("Color"));
        assert_eq!(st.text(110).as_deref(), Some("Blue"));
        assert_eq!(st.text(102).as_deref(), Some("Bar - y"));
        assert_eq!(st.text(112).as_deref(), Some("2"));
        assert_eq!(st.text(113), None);
        assert_eq!(st.button(175), Some(1));
        assert_eq!(st.button(170), Some(0));
        assert_eq!(st.rate(7), 0.0);
        // another type: buttons 170-185
        s.event(&mut ctx, 176, 1);
        assert_eq!(s.kind(), SkinType::Decide);
        ctx.save_config();
        let back = crate::config::Config::load(&ctx.paths.config_file());
        std::fs::remove_dir_all(&root).ok();
        assert_eq!(back.skins["SELECT"], "skins/b/select.json");
        assert_eq!(back.skin_options["skins/b/select.json"]["Color"], "Blue");
        assert_eq!(back.skin_offsets["skins/b/select.json"]["Bar"][1], 2);
    }
}
