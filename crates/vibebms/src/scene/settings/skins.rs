use super::{mb, Act, Armed, Confirm, Row};
use crate::app::Ctx;
use crate::platform::IS_WEB;
use crate::skinstore::{SkinEntry, State};
use std::path::{Path, PathBuf};

/// The browser leaves out what it cannot install (the largest skins).
fn shown(e: &SkinEntry) -> bool {
    !IS_WEB || e.web_refusal().is_none()
}

/// The skin catalog: each skin, what it is, and what can be done with it.
pub(super) fn rows(ctx: &Ctx, armed: &Armed<Confirm>) -> Vec<Row> {
    let store = &ctx.skins.store;
    let mut v = vec![Row::new(
        format!(
            "Skin catalog: {} skins, installed into {}",
            store.catalog.skins.iter().filter(|e| shown(e)).count(),
            ctx.paths.skins.display()
        ),
        if store.is_fetching() {
            "reading…"
        } else {
            "update ↻"
        },
        Act::Run(|c, _| c.skins.store.refresh(true), 0),
    )];
    if IS_WEB {
        v.push(Row::note(
            "Skins over 400 MB are in the desktop and mobile apps",
            "",
        ));
    }
    for (i, e) in store.catalog.skins.iter().enumerate() {
        if !shown(e) {
            continue;
        }
        let state = store.state(&ctx.paths.skins, e);
        let size = if e.size > 0 {
            format!(" ({})", mb(e.size))
        } else {
            String::new()
        };
        let value = match &state {
            State::Busy(p) => format!("{}  cancel ×", p.label()),
            State::Installed { update: true } => format!("update ↓{size}"),
            State::Installed { .. } if armed.is(&Confirm::Reinstall(e.id.clone())) => {
                "press again to reinstall".into()
            }
            State::Installed { .. } => "installed ✓".into(),
            State::Absent if IS_WEB && e.web_refusal().is_some() => {
                "desktop / mobile app only".into()
            }
            State::Absent => format!("install ↓{size}"),
        };
        let version = if e.version.is_empty() {
            String::new()
        } else {
            format!(" {}", e.version)
        };
        v.push(Row::new(
            format!("{}{version}  by {}", e.name, e.author),
            value,
            Act::SkinGet(i),
        ));
        let kind = if e.kind == "lr2" {
            "LR2"
        } else if e.lua {
            "beatoraja Lua"
        } else {
            "beatoraja"
        };
        let mut info = format!("    {kind} · {}", e.scenes_label());
        if !e.resolution.is_empty() {
            info.push_str(&format!(" · {}", e.resolution));
        }
        v.push(Row::new(
            info,
            "page ↗",
            Act::Run(
                |c, i| {
                    if let Some(e) = c.skins.store.catalog.skins.get(i) {
                        crate::platform::open_url(&e.homepage);
                    }
                },
                i,
            ),
        ));
        // the author's terms, on a line of their own (they can be long)
        if !e.license.is_empty() {
            v.push(Row::note(format!("    Terms: {}", e.license), ""));
        }
        if matches!(state, State::Installed { .. }) {
            v.push(Row::new(
                format!("    skins/{}", e.folder()),
                if armed.is(&Confirm::DeleteSkin(e.id.clone())) {
                    "press again to delete"
                } else {
                    "delete ×"
                },
                Act::SkinDelete(i),
            ));
        }
    }
    v
}

/// Install catalog skin `i`: an installed one again on a second press, a
/// downloading one is cancelled.
pub(super) fn get(ctx: &mut Ctx, armed: &mut Armed<Confirm>, i: usize) {
    let Some(e) = ctx.skins.store.catalog.skins.get(i).cloned() else {
        return;
    };
    match ctx.skins.store.state(&ctx.paths.skins, &e) {
        State::Busy(_) => {
            ctx.skins.store.cancel(&e.id);
            return;
        }
        State::Installed { update: false } if !armed.confirm(Confirm::Reinstall(e.id)) => return,
        _ => armed.clear(),
    }
    let r = ctx.skins.store.install(&ctx.paths, &ctx.net.hosts, i);
    ctx.toast(r.unwrap_or_else(|e| e));
}

/// Delete catalog skin `i`, on a second press; the skins chosen from its
/// folders go back to the default.
pub(super) fn delete(ctx: &mut Ctx, armed: &mut Armed<Confirm>, i: usize) {
    let Some(e) = ctx.skins.store.catalog.skins.get(i).cloned() else {
        return;
    };
    if !armed.confirm(Confirm::DeleteSkin(e.id.clone())) {
        return;
    }
    match ctx.skins.store.delete(&ctx.paths, i) {
        Ok(folders) => {
            let skins = &ctx.paths.skins;
            let rel = skins
                .strip_prefix(&ctx.paths.root)
                .map(Path::to_path_buf)
                .unwrap_or_else(|_| skins.clone());
            let gone: Vec<PathBuf> = folders
                .iter()
                .flat_map(|f| [rel.join(f), skins.join(f)])
                .collect();
            for v in ctx.config.skins.values_mut() {
                if gone.iter().any(|g| Path::new(v.as_str()).starts_with(g)) {
                    v.clear();
                }
            }
            ctx.skins.files_changed();
            ctx.toast(format!("Deleted {}", e.name));
        }
        Err(m) => ctx.toast(m),
    }
}
