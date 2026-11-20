use super::files::{filepath_chosen, skin_path};
use super::*;

/// Run a `.luaskin` and return its table as JSON (functions become
/// script markers) with the Lua state. `skin_config` (option name ->
/// selected op) is exported like beatoraja does.
pub fn eval(
    path: &Path,
    home: &Home,
    skin_config: Option<SkinConfig<'_>>,
    state: Option<&dyn SkinState>,
) -> Result<(Value, script::Script), String> {
    let src = vfs::read(path).map_err(|e| e.to_string())?;
    let e = |e: mlua::Error| format!("{}: {e}", path.display());
    let mut sc = script::Script::new(home, false).map_err(e)?;
    let lua = sc.lua().clone();
    let g = lua.globals();
    if let Some((opts, header, user_offsets, user_files)) = skin_config {
        let cfg = lua.create_table().map_err(e)?;
        let option = lua.create_table().map_err(e)?;
        let enabled = lua.create_table().map_err(e)?;
        for (k, v) in opts {
            option.set(k.as_str(), *v).map_err(e)?;
            enabled.push(*v).map_err(e)?;
        }
        cfg.set("option", option).map_err(e)?;
        cfg.set("enabled_options", enabled).map_err(e)?;
        // custom offsets (the user's values) and the files `filepath`
        // customisations pick
        let offsets = lua.create_table().map_err(e)?;
        for name in &header.offset_names {
            let t = lua.create_table().map_err(e)?;
            let v = user_offsets.get(name).copied().unwrap_or_default();
            for (k, x) in ["x", "y", "w", "h", "r", "a"].iter().zip(v) {
                t.set(*k, x).map_err(e)?;
            }
            offsets.set(name.as_str(), t).map_err(e)?;
        }
        cfg.set("offset", offsets).map_err(e)?;
        let fps = header.filepaths.clone();
        let files = lua.create_table().map_err(e)?;
        for f in &fps {
            if let Some(name) = filepath_chosen(home, f, user_files) {
                files.set(f.name.as_str(), name).map_err(e)?;
            }
        }
        cfg.set("file_path", files).map_err(e)?;
        let (home, chosen) = (home.clone(), user_files.clone());
        cfg.set(
            "get_path",
            // (a path out of the theme: none)
            lua.create_function(move |_, p: String| {
                let found = skin_path(&home, &fps, &chosen, &p);
                Ok(found.map_or_else(String::new, |p| p.to_string_lossy().into_owned()))
            })
            .map_err(e)?,
        )
        .map_err(e)?;
        g.set("skin_config", cfg).map_err(e)?;
    }
    let v: mlua::Value = sc
        .with_state(state, || script::text_chunk(&lua, &src, path).eval())
        .map_err(e)?;
    let json = sc.to_json(v).map_err(e)?;
    Ok((json, sc))
}
