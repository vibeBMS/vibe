pub mod decide;
pub mod graphs;
pub mod keybind;
pub mod keyconfig;
pub mod option_panel;
pub mod play;
pub mod play_opts;
pub mod play_skin;
pub mod practice;
pub mod result;
pub mod result_skin;
pub mod select;
pub mod settings;
pub mod skin_host;
pub mod skinned;
pub mod skinselect;
pub mod skinvals;
pub mod target;
pub mod touch_layout;

/// Position of `v` in `all` (an image set index).
pub fn index_of<T: PartialEq>(all: &[T], v: T) -> i64 {
    all.iter().position(|x| *x == v).unwrap_or(0) as i64
}

/// `v` moved `d` places along `all`, around the end.
pub fn cycle<T: Copy + PartialEq>(all: &[T], v: T, d: i32) -> T {
    let n = all.len() as i32;
    let i = all.iter().position(|x| *x == v).unwrap_or(0) as i32;
    all[(i + d).rem_euclid(n) as usize]
}

/// ON / OFF as skins and toasts show them (untranslated: settings rows
/// have their own, `settings::row::onoff`).
pub fn onoff(b: bool) -> &'static str {
    if b {
        "ON"
    } else {
        "OFF"
    }
}
