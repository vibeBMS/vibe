//! beatoraja skin scripts: values given as Lua functions (Lua skins) or Lua
//! source strings (JSON skins) — draw conditions, timers, numbers, texts,
//! image indices, slider / graph rates and click events.
//!
//! Each script value gets a synthetic id (`SCRIPT_ID + n`) in the namespace
//! of what it replaces (option, timer, number, ...). [`Overlay`] answers
//! those ids, running a script against the scene's [`SkinState`] when its
//! value is first read in a frame ([`Runtime::update`] starts one).
//! Scripts reach the game through beatoraja's `main_state`, `timer_util`
//! and `event_util` modules (as globals in JSON skins).

use super::{Home, SkinState};
use std::collections::HashMap;

/// First synthetic id of a script value.
pub const SCRIPT_ID: i32 = 1 << 24;

/// What a script value stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptKind {
    Bool,
    Int,
    Float,
    Str,
    /// Start time in µs, `math.mininteger` when off.
    Timer,
    /// Runs on click / when triggered, no value.
    Event,
}

/// A frame's result of one script.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Val {
    #[default]
    Nil,
    Bool(bool),
    Int(i64),
    Num(f64),
    Str(String),
}

impl Val {
    fn truthy(&self) -> bool {
        !matches!(self, Val::Nil | Val::Bool(false))
    }
    fn int(&self) -> Option<i64> {
        match self {
            Val::Int(i) => Some(*i),
            Val::Num(n) => Some(*n as i64),
            Val::Str(s) => s.trim().parse().ok(),
            Val::Bool(b) => Some(*b as i64),
            Val::Nil => None,
        }
    }
    fn num(&self) -> f64 {
        match self {
            Val::Int(i) => *i as f64,
            Val::Num(n) => *n,
            Val::Str(s) => s.trim().parse().unwrap_or(0.0),
            Val::Bool(b) => *b as i64 as f64,
            Val::Nil => 0.0,
        }
    }
    fn text(&self) -> String {
        match self {
            Val::Int(i) => i.to_string(),
            Val::Num(n) => n.to_string(),
            Val::Str(s) => s.clone(),
            Val::Bool(b) => b.to_string(),
            Val::Nil => String::new(),
        }
    }
}

fn script_index(id: i32) -> Option<usize> {
    (id >= SCRIPT_ID).then(|| (id - SCRIPT_ID) as usize)
}

/// A scene's state with the skin's own values on top: script results and
/// the user's offsets.
pub struct Overlay<'a> {
    pub st: &'a dyn SkinState,
    pub rt: Option<&'a Runtime>,
    pub offsets: &'a HashMap<i32, [f32; 6]>,
}

impl Overlay<'_> {
    /// `f` of script `id`'s value (`None`: not a script).
    fn val<R>(&self, id: i32, f: impl FnOnce(&Val) -> R) -> Option<R> {
        let (rt, k) = (self.rt?, script_index(id)?);
        // scripts see the state under the skin's values, as in `update`
        let st = Overlay {
            st: self.st,
            rt: None,
            offsets: self.offsets,
        };
        Some(rt.value(k, &st, f))
    }
}

impl SkinState for Overlay<'_> {
    fn timer(&self, id: i32) -> Option<i64> {
        if let Some(us) = self.rt.and_then(|rt| rt.timer_override(id)) {
            return us;
        }
        match self.val(id, Val::int) {
            Some(Some(us)) if us != i64::MIN => Some(us.div_euclid(1000)),
            Some(_) => None,
            None => self.st.timer(id),
        }
    }
    fn option(&self, id: i32) -> bool {
        match self.val(id.abs(), Val::truthy) {
            Some(on) => on == (id > 0),
            None => self.st.option(id),
        }
    }
    fn number(&self, id: i32) -> Option<i64> {
        self.val(id, Val::int).unwrap_or_else(|| self.st.number(id))
    }
    fn text(&self, id: i32) -> Option<String> {
        self.val(id, Val::text).or_else(|| self.st.text(id))
    }
    fn rate(&self, id: i32) -> f32 {
        self.val(id, |v| v.num() as f32)
            .unwrap_or_else(|| self.st.rate(id))
    }
    fn button(&self, id: i32) -> Option<i64> {
        self.val(id, Val::int).unwrap_or_else(|| self.st.button(id))
    }
    fn distribution(&self, rank: bool) -> Option<Vec<u32>> {
        self.st.distribution(rank)
    }
    fn offset(&self, id: i32) -> Option<[f32; 6]> {
        self.offsets
            .get(&id)
            .copied()
            .or_else(|| self.st.offset(id))
    }
}

pub use lua::*;

/// The Lua runtime (on the web, Lua comes from vibebms-lua-web).
mod lua {
    use super::*;
    use mlua::{Function, IntoLuaMulti, Lua, Table, Value as LV};
    use serde_json::Value;
    use std::cell::{Cell, RefCell};
    use std::collections::{HashMap, HashSet};
    use std::path::Path;
    use std::rc::Rc;

    /// Marker a Lua function is replaced with in the skin table.
    const FN_MARK: &str = "\u{1}fn:";

    /// `main_state` (forwarding to the per-frame state table), `timer_util`
    /// and `event_util`, after beatoraja's.
    const PRELUDE: &str = r#"
local vb, global = ...
local OFF = math.mininteger
local names = { "option", "number", "float_number", "text", "offset", "timer",
  "timer_is_on", "timer_is_off", "timer_elapsed", "timer_elapsed_ms",
  "timer_elapsed_seconds", "time", "set_timer", "event_exec", "event_index",
  "rate", "exscore", "rate_best", "exscore_best", "rate_rival", "exscore_rival",
  "gauge", "gauge_type", "judge" }
local ms = { timer_off_value = OFF }
for _, k in ipairs(names) do ms[k] = function(...) return vb[k](...) end end
-- no network from skins: what beatoraja answers with it off (a skin
-- polling a request sees it fail instead of waiting for ever)
ms.open_url = function() return false end
ms.http_get = function() return nil, "HTTP disabled" end
ms.http_get_lines = ms.http_get
ms.http_get_async = function() return nil, "HTTP disabled" end
ms.http_poll = function()
  return { status = "failed", status_code = 0, error = "HTTP disabled", lines = {} }
end
ms.http_cancel = function() return false end
-- anything else (input, audio, volume ...) is a harmless no-op
setmetatable(ms, { __index = function() return function() return 0 end end })
ms.timer_is_on = function(id) return vb.timer(id) ~= OFF end
ms.timer_is_off = function(id) return vb.timer(id) == OFF end
ms.timer_elapsed = function(id) local t = vb.timer(id); if t == OFF then return -1 end return vb.time() - t end
ms.timer_elapsed_ms = function(id) local t = vb.timer(id); if t == OFF then return -1 end return (vb.time() - t) // 1000 end
ms.timer_elapsed_seconds = function(id) local t = vb.timer(id); if t == OFF then return -1 end return (vb.time() - t) / 1000000 end

local tu = {}
function tu.now_timer(t) if t ~= OFF then return ms.time() - t end return 0 end
function tu.is_timer_on(t) return t ~= OFF end
function tu.is_timer_off(t) return t == OFF end
function tu.timer_function(id) return function() return ms.timer(id) end end
function tu.timer_observe_boolean(f)
  local v = OFF
  return function()
    local on = f()
    if on and v == OFF then v = ms.time() elseif not on and v ~= OFF then v = OFF end
    return v
  end
end
function tu.new_passive_timer()
  local v = OFF
  return {
    timer = function() return v end,
    turn_on = function() if v == OFF then v = ms.time() end return true end,
    turn_on_reset = function() v = ms.time() return true end,
    turn_off = function() v = OFF return true end,
  }
end

local eu = {}
function eu.event_observe_turn_true(f, act)
  local on = false
  return function()
    local n = f() and true or false
    if on ~= n then on = n; if on then act() end end
    return true
  end
end
function eu.event_observe_timer(tf, act)
  local v = OFF
  return function()
    local n = tf()
    if n ~= v and n ~= OFF then v = n; act() end
    return true
  end
end
function eu.event_observe_timer_on(tf, act)
  local on = false
  return function()
    local n = tf() ~= OFF
    if on ~= n then on = n; if on then act() end end
    return true
  end
end
function eu.event_observe_timer_off(tf, act)
  local off = false
  return function()
    local n = tf() == OFF
    if off ~= n then off = n; if off then act() end end
    return true
  end
end
function eu.event_min_interval(iv, act)
  local last = OFF
  return function()
    local now = ms.time()
    if last == OFF or (now - last) // 1000 >= iv then last = now; act() end
    return true
  end
end

package.preload["main_state"] = function() return ms end
package.preload["timer_util"] = function() return tu end
package.preload["event_util"] = function() return eu end
if global then
  for k, v in pairs(ms) do _G[k] = v end
  for _, k in ipairs(names) do _G[k] = ms[k] end
  _G.timer_off_value = OFF
  for k, v in pairs(tu) do _G[k] = v end
  for k, v in pairs(eu) do _G[k] = v end
end
"#;

    /// What beatoraja's Lua (LuaJ, Lua 5.2) has and Lua 5.4 lacks or does
    /// differently: `os` without what reaches the system, `bit32`,
    /// `unpack`, number arguments that need not be integers, and inert
    /// `luajava` / `debug` as beatoraja gives old skins.
    const COMPAT: &str = r#"
for _, k in ipairs({ "execute", "exit", "getenv", "remove", "rename", "tmpname", "setlocale" }) do
  os[k] = nil
end
unpack = unpack or table.unpack
loadstring = loadstring or load
math.pow = math.pow or function(a, b) return a ^ b end
local function int(x)
  if math.type(x) == "float" and x == x and x ~= math.huge and x ~= -math.huge then
    return x >= 0 and math.floor(x) or math.ceil(x)
  end
  return x
end
local random = math.random
math.random = function(m, n)
  if m == nil then return random() end
  if n == nil then return random(int(m)) end
  return random(int(m), int(n))
end
local format = string.format
local INT = { d = true, i = true, x = true, X = true, o = true, c = true }
string.format = function(fmt, ...)
  local args = table.pack(...)
  if type(fmt) == "string" then
    local k = 0
    for conv in fmt:gmatch("%%[-+ #0]*%d*%.?%d*([%a%%])") do
      if conv ~= "%" then
        k = k + 1
        if INT[conv] then args[k] = int(args[k]) end
      end
    end
  end
  return format(fmt, table.unpack(args, 1, args.n))
end

local M = 0xFFFFFFFF
local function u(x) return int(x or 0) & M end
local b = {}
function b.band(...) local r = M for _, v in ipairs({ ... }) do r = r & u(v) end return r end
function b.bor(...) local r = 0 for _, v in ipairs({ ... }) do r = r | u(v) end return r end
function b.bxor(...) local r = 0 for _, v in ipairs({ ... }) do r = r ~ u(v) end return r end
function b.bnot(x) return ~u(x) & M end
function b.btest(...) return b.band(...) ~= 0 end
function b.lshift(x, n)
  n = int(n)
  if n < 0 then return b.rshift(x, -n) end
  if n >= 32 then return 0 end
  return (u(x) << n) & M
end
function b.rshift(x, n)
  n = int(n)
  if n < 0 then return b.lshift(x, -n) end
  if n >= 32 then return 0 end
  return u(x) >> n
end
function b.arshift(x, n)
  x, n = u(x), int(n)
  if n < 0 then return b.lshift(x, -n) end
  local neg = x & 0x80000000 ~= 0
  if n >= 32 then return neg and M or 0 end
  local r = x >> n
  if neg then r = r | ((M << (32 - n)) & M) end
  return r
end
function b.lrotate(x, n)
  x, n = u(x), int(n) % 32
  return ((x << n) | (x >> (32 - n))) & M
end
function b.rrotate(x, n) return b.lrotate(x, -int(n)) end
function b.extract(x, f, w)
  w = int(w or 1)
  return (u(x) >> int(f)) & ((1 << w) - 1)
end
function b.replace(x, v, f, w)
  f, w = int(f), int(w or 1)
  local m = ((1 << w) - 1) << f
  return ((u(x) & ~m) | ((u(v) << f) & m)) & M
end
bit32 = b
package.loaded.bit32 = b

-- beatoraja's stand-ins for old skins: Java classes are inert here
local inert = setmetatable({}, { __call = function() return nil end })
getmetatable(inert).__index = function() return inert end
luajava = {
  bindClass = function() return inert end,
  new = function() return inert end,
  newInstance = function() return inert end,
  createProxy = function() return inert end,
}
package.loaded.luajava = luajava
debug = { getmetatable = getmetatable }
package.loaded.debug = debug
"#;

    /// `io` over the skin's files (read through `read`): files open for
    /// writing take what is written and keep nothing.
    const IO: &str = r#"
local read = ...
local F = {}
F.__index = F
local function file(data) return setmetatable({ data = data or "", pos = 1 }, F) end
function F:read(...)
  local fmts, out = table.pack(...), {}
  if fmts.n == 0 then fmts = { "l", n = 1 } end
  for k = 1, fmts.n do
    local f, v = fmts[k], nil
    if type(f) == "string" then f = f:gsub("^%*", ""):sub(1, 1) end
    if type(f) == "number" then
      if self.pos <= #self.data or f == 0 then
        v = self.data:sub(self.pos, self.pos + f - 1)
        self.pos = self.pos + #v
      end
    elseif f == "a" then
      v = self.data:sub(self.pos)
      self.pos = #self.data + 1
    elseif f == "l" or f == "L" then
      if self.pos <= #self.data then
        local e = self.data:find("\n", self.pos, true) or #self.data + 1
        v = self.data:sub(self.pos, f == "L" and e or e - 1)
        self.pos = e + 1
        if f == "l" then v = v:gsub("\r$", "") end
      end
    elseif f == "n" then
      local s, e = self.data:find("^%s*[-+]?[%d.]+[eE]?[-+]?%d*", self.pos)
      if s then
        v = tonumber(self.data:sub(s, e):match("%S+"))
        self.pos = e + 1
      end
    end
    out[k] = v
    if v == nil then return table.unpack(out, 1, k) end
  end
  return table.unpack(out, 1, fmts.n)
end
function F:lines(...)
  local fmts = table.pack(...)
  return function() return self:read(table.unpack(fmts, 1, fmts.n)) end
end
function F:write() return self end
function F:flush() return self end
function F:setvbuf() return true end
function F:close() self.closed = true return true end
function F:seek(whence, off)
  local base = ({ set = 0, cur = self.pos - 1, ["end"] = #self.data })[whence or "cur"] or 0
  self.pos = math.max(0, math.min(#self.data, base + (off or 0))) + 1
  return self.pos - 1
end
io = {
  stdin = file(), stdout = file(), stderr = file(),
  open = function(path, mode)
    mode = mode or "r"
    if mode:find("r") then
      local d = read(tostring(path))
      if d == nil then return nil, tostring(path) .. ": No such file or directory", 2 end
      return file(d)
    end
    return file()
  end,
  lines = function(path, ...)
    if path == nil then return function() return nil end end
    local d = read(tostring(path))
    if d == nil then error(tostring(path) .. ": No such file or directory", 2) end
    return file(d):lines(...)
  end,
  read = function() return nil end,
  write = function() return io.stdout end,
  close = function(f) if f then return f:close() end return true end,
  input = function() return io.stdin end,
  output = function() return io.stdout end,
  tmpfile = function() return file() end,
  popen = function() return nil, "not allowed" end,
  type = function(f)
    if getmetatable(f) ~= F then return nil end
    return f.closed and "closed file" or "file"
  end,
}
package.loaded.io = io
"#;

    /// Largest file a script may read.
    const IO_MAX_BYTES: i64 = 16 << 20;

    /// The file a skin script names (`require`, `dofile`, `io`), inside
    /// the theme folder `root`. Paths are relative to the skin's folder or
    /// to beatoraja's folder (`skin/<theme>/...`, whatever the theme
    /// folder is called here); `lua_only` admits Lua files only.
    fn resolve(dir: &Path, root: &Path, name: &str, lua_only: bool) -> Option<std::path::PathBuf> {
        use crate::platform::vfs;
        let name = name.replace('\\', "/");
        // (the folder a name is taken from, the name)
        let mut cands = vec![(Path::new(""), name.as_str())];
        if !Path::new(&name).is_absolute() {
            let tail = name
                .get(..5)
                .filter(|h| h.eq_ignore_ascii_case("skin/"))
                .and_then(|_| name[5..].split_once('/'))
                .map(|(_, t)| t);
            for a in dir.ancestors() {
                cands.push((a, &name));
                if let Some(t) = tail {
                    cands.push((a, t));
                }
                if vfs::normalize(a) == *root {
                    break;
                }
            }
        }
        cands
            .into_iter()
            // a name that leads out of the theme is dropped before anything
            // is looked for on the disk
            .filter_map(|(from, name)| vfs::within(root, from, name))
            .map(|c| vfs::resolve_case(&c))
            .filter(|c| {
                !lua_only
                    || c.extension().is_some_and(|e| {
                        e.eq_ignore_ascii_case("lua") || e.eq_ignore_ascii_case("luaskin")
                    })
            })
            .find(|c| vfs::is_file(c) && really_inside(c, root))
    }

    /// `path` is inside `root` once links are followed (a link in the skin
    /// folder must not reach other files).
    fn really_inside(path: &Path, root: &Path) -> bool {
        // the web's file system has no links (nor `canonicalize`)
        if cfg!(target_arch = "wasm32") {
            return true;
        }
        match (std::fs::canonicalize(path), std::fs::canonicalize(root)) {
            (Ok(p), Ok(r)) => p.starts_with(r),
            _ => false,
        }
    }

    /// Replace the functions of the skin table with markers (indices into
    /// the returned list), so the table converts to JSON.
    const WALK: &str = r#"
local root, mark = ...
local fns, seen = {}, {}
local function walk(t)
  if seen[t] then return end
  seen[t] = true
  local named, numbered = false, {}
  for k, v in pairs(t) do
    if type(v) == "function" then
      fns[#fns + 1] = v
      t[k] = mark .. #fns
    elseif type(v) == "table" then
      walk(v)
    end
    if type(k) == "number" then numbered[#numbered + 1] = k else named = true end
  end
  -- a table with names and positional entries is an object (as beatoraja
  -- reads it), not an array: give the positional entries string keys
  if named and #numbered > 0 then
    for _, k in ipairs(numbered) do
      t[tostring(k)] = t[k]
      t[k] = nil
    end
  end
end
if type(root) == "table" then walk(root) end
return fns
"#;

    /// Lua memory per skin.
    const MEMORY_LIMIT: usize = 256 << 20;
    /// Time a skin may take to load, and its scripts per frame.
    #[cfg(not(test))]
    const LOAD_TIME: std::time::Duration = std::time::Duration::from_secs(10);
    #[cfg(test)]
    const LOAD_TIME: std::time::Duration = std::time::Duration::from_millis(300);
    const FRAME_TIME: std::time::Duration = std::time::Duration::from_secs(1);

    /// When running scripts are stopped.
    struct Deadline(web_time::Instant);

    const TOO_LONG: &str = "skin script ran too long";

    fn expired(lua: &Lua) -> bool {
        lua.app_data_ref::<Deadline>()
            .is_some_and(|d| web_time::Instant::now() > d.0)
    }

    /// `pcall`, `xpcall` and `coroutine.resume` / `wrap` would catch the
    /// error that stops a script past its time and let it go on: past it,
    /// they pass the error on.
    const GUARD: &str = r#"
local expired, too_long = ...
local pcall0, xpcall0, create, resume0 = pcall, xpcall, coroutine.create, coroutine.resume
local function check(...)
  if expired() then error(too_long, 0) end
  return ...
end
pcall = function(...) return check(pcall0(...)) end
xpcall = function(...) return check(xpcall0(...)) end
coroutine.resume = function(...) return check(resume0(...)) end
coroutine.wrap = function(f)
  local co = create(f)
  return function(...)
    local r = table.pack(check(resume0(co, ...)))
    if r[1] then return table.unpack(r, 2, r.n) end
    error(r[2], 0)
  end
end
"#;

    /// `string.find`, `match`, `gmatch` and `gsub` by `luapat` (a step
    /// budget per call; Lua's own run in C where the hook cannot stop them).
    fn install_patterns(lua: &Lua) -> mlua::Result<()> {
        use super::super::luapat::{self, Cap};
        fn text(lua: &Lua, v: LV, n: usize, f: &str) -> mlua::Result<Vec<u8>> {
            let tn = v.type_name();
            match lua.coerce_string(v)? {
                Some(s) => Ok(s.as_bytes().to_vec()),
                None => Err(mlua::Error::runtime(format!(
                    "bad argument #{n} to '{f}' (string expected, got {tn})"
                ))),
            }
        }
        fn caps(lua: &Lua, c: &[Cap]) -> mlua::Result<Vec<LV>> {
            c.iter()
                .map(|c| match c {
                    Cap::Str(b) => lua.create_string(b).map(LV::String),
                    Cap::Pos(n) => Ok(LV::Integer(*n)),
                })
                .collect()
        }
        fn err(e: String) -> mlua::Error {
            mlua::Error::runtime(e)
        }
        fn truthy(v: &LV) -> bool {
            !matches!(v, LV::Nil | LV::Boolean(false))
        }
        let string: Table = lua.globals().get("string")?;
        for (name, is_find) in [("find", true), ("match", false)] {
            let f =
                lua.create_function(move |lua, (s, p, init, plain): (LV, LV, Option<i64>, LV)| {
                    let (s, p) = (text(lua, s, 1, name)?, text(lua, p, 2, name)?);
                    let hit = luapat::find(&s, &p, init.unwrap_or(1), truthy(&plain), is_find)
                        .map_err(err)?;
                    let mut out = Vec::new();
                    match hit {
                        None => out.push(LV::Nil),
                        Some((a, b, c)) => {
                            if is_find {
                                out.push(LV::Integer(a as i64));
                                out.push(LV::Integer(b as i64));
                            }
                            out.extend(caps(lua, &c)?);
                        }
                    }
                    Ok(mlua::MultiValue::from_iter(out))
                })?;
            string.set(name, f)?;
        }
        let gmatch = lua.create_function(|lua, (s, p, init): (LV, LV, Option<i64>)| {
            let (s, p) = (text(lua, s, 1, "gmatch")?, text(lua, p, 2, "gmatch")?);
            let pos = luapat::gmatch_start(init.unwrap_or(1), s.len());
            let state = RefCell::new((pos, None));
            lua.create_function(move |lua, ()| {
                let mut st = state.borrow_mut();
                let (pos, last) = &mut *st;
                match luapat::gmatch_next(&s, &p, pos, last).map_err(err)? {
                    Some(c) => Ok(mlua::MultiValue::from_iter(caps(lua, &c)?)),
                    None => Ok(mlua::MultiValue::from_iter([LV::Nil])),
                }
            })
        })?;
        string.set("gmatch", gmatch)?;
        let gsub = lua.create_function(|lua, (s, p, repl, n): (LV, LV, LV, Option<i64>)| {
            let (s, p) = (text(lua, s, 1, "gsub")?, text(lua, p, 2, "gsub")?);
            // what a function or a table gives for a match: nil / false keep it
            let value = |v: LV| -> Result<Option<Vec<u8>>, String> {
                match v {
                    LV::Nil | LV::Boolean(false) => Ok(None),
                    LV::String(_) | LV::Integer(_) | LV::Number(_) => lua
                        .coerce_string(v)
                        .map_err(|e| e.to_string())
                        .map(|s| s.map(|s| s.as_bytes().to_vec())),
                    v => Err(format!("invalid replacement value (a {})", v.type_name())),
                }
            };
            let (out, count) = match repl {
                LV::String(_) | LV::Integer(_) | LV::Number(_) => {
                    let r = text(lua, repl, 3, "gsub")?;
                    luapat::gsub(&s, &p, luapat::Repl::Str(&r), n)
                }
                LV::Table(t) => {
                    let mut f = |c: &[Cap]| {
                        let k = caps(lua, &c[..1]).map_err(|e| e.to_string())?;
                        value(t.get(k.into_iter().next()).map_err(|e| e.to_string())?)
                    };
                    luapat::gsub(&s, &p, luapat::Repl::Call(&mut f), n)
                }
                LV::Function(fun) => {
                    let mut f = |c: &[Cap]| {
                        let args = caps(lua, c).map_err(|e| e.to_string())?;
                        let r: LV = fun
                            .call(mlua::MultiValue::from_iter(args))
                            .map_err(|e| e.to_string())?;
                        value(r)
                    };
                    luapat::gsub(&s, &p, luapat::Repl::Call(&mut f), n)
                }
                v => {
                    return Err(err(format!(
                        "bad argument #3 to 'gsub' (string/function/table expected, got {})",
                        v.type_name()
                    )))
                }
            }
            .map_err(err)?;
            Ok((lua.create_string(out)?, count))
        })?;
        string.set("gsub", gsub)?;
        Ok(())
    }

    fn arm(lua: &Lua, budget: std::time::Duration) {
        lua.set_app_data(Deadline(web_time::Instant::now() + budget));
    }

    /// A Lua source file as a chunk (never compiled bytecode).
    pub fn text_chunk<'a>(lua: &'a Lua, src: &'a [u8], path: &Path) -> mlua::chunk::Chunk<'a> {
        lua.load(src)
            .set_name(path.display().to_string())
            .set_mode(mlua::chunk::ChunkMode::Text)
    }

    /// The Lua state of a skin while it loads.
    pub struct Script {
        lua: Lua,
        frame: Rc<Frame>,
        fns: Option<Table>,
    }

    impl Script {
        /// A Lua state with the skin modules; `global` also exposes them as
        /// globals (JSON skins), `home` is where the skin's files are.
        pub fn new(home: &Home, global: bool) -> mlua::Result<Script> {
            // no debug, and os / io as beatoraja restricts them (below):
            // scripts only reach what the skin modules give
            let libs = mlua::StdLib::COROUTINE
                | mlua::StdLib::TABLE
                | mlua::StdLib::STRING
                | mlua::StdLib::UTF8
                | mlua::StdLib::MATH
                | mlua::StdLib::OS
                | mlua::StdLib::PACKAGE;
            #[cfg(target_arch = "wasm32")]
            vibebms_lua_web::linked();
            let lua = Lua::new_with(libs, mlua::LuaOptions::default())?;
            lua.set_memory_limit(MEMORY_LIMIT)?;
            // global: coroutines the scripts create run under it too (a
            // thread hook is dropped in them)
            lua.set_global_hook(
                mlua::HookTriggers::new().every_nth_instruction(10_000),
                |lua, _| match expired(lua) {
                    true => Err(mlua::Error::runtime(TOO_LONG)),
                    false => Ok(mlua::VmState::Continue),
                },
            )?;
            arm(&lua, LOAD_TIME);
            // text chunks only: Lua does not verify compiled bytecode
            lua.load(
                r##"local load = load
                load = function(chunk, name, _, ...)
                    if select("#", ...) > 0 then return load(chunk, name, "t", ...) end
                    return load(chunk, name, "t")
                end
                string.dump = nil"##,
            )
            .call::<()>(())?;
            lua.load(COMPAT).set_name("vibebms compat").call::<()>(())?;
            install_patterns(&lua)?;
            let expired_fn = lua.create_function(|lua, ()| Ok(expired(lua)))?;
            lua.load(GUARD)
                .set_name("vibebms guard")
                .call::<()>((expired_fn, TOO_LONG))?;
            let vb = lua.create_table()?;
            let frame = Rc::new(Frame::default());
            install(&lua, &vb, &frame)?;
            // `require`, `dofile`, `loadfile` and `io` read files of the skin's theme
            // folder only
            let root = crate::platform::vfs::normalize(&home.theme);
            let base = home.dir.clone();
            let find = {
                let (root, base) = (root.clone(), base.clone());
                move |p: &str, lua_only: bool| resolve(&base, &root, p, lua_only)
            };
            let pkg: Table = lua.globals().get("package")?;
            for k in ["path", "cpath"] {
                pkg.set(k, "")?;
            }
            for k in ["loadlib", "searchpath"] {
                pkg.set(k, LV::Nil)?;
            }
            let f = find.clone();
            let searcher = lua.create_function(move |lua, name: String| {
                let rel = name.replace('.', "/");
                // the skin's folder, then `package.path` as the skin set it
                let pkg: Table = lua.globals().get("package")?;
                let templates: String = pkg.get::<Option<String>>("path")?.unwrap_or_default();
                let found = ["?.lua", "?.luaskin", "?/init.lua"]
                    .into_iter()
                    .chain(templates.split(';'))
                    .filter(|t| t.contains('?'))
                    .find_map(|t| f(&t.replace('?', &rel), true));
                let Some(path) = found else {
                    return Ok(LV::String(
                        lua.create_string(format!("no skin file {name}"))?,
                    ));
                };
                let src = crate::platform::vfs::read(&path)
                    .map_err(|e| mlua::Error::runtime(e.to_string()))?;
                Ok(LV::Function(text_chunk(lua, &src, &path).into_function()?))
            })?;
            let preload: LV = pkg.get::<Table>("searchers")?.raw_get(1)?;
            let searchers = lua.create_table()?;
            searchers.raw_set(1, preload)?;
            searchers.raw_set(2, searcher)?;
            pkg.set("searchers", searchers)?;
            // `loadfile` too (the base library's would read any path and
            // take compiled bytecode, which Lua does not verify)
            let f = find.clone();
            lua.globals().set(
                "loadfile",
                lua.create_function(move |lua, p: String| {
                    let Some(path) = f(&p, true) else {
                        return Ok((LV::Nil, Some(format!("loadfile: no skin file {p}"))));
                    };
                    let src = crate::platform::vfs::read(&path)
                        .map_err(|e| mlua::Error::runtime(e.to_string()))?;
                    match text_chunk(lua, &src, &path).into_function() {
                        Ok(f) => Ok((LV::Function(f), None)),
                        Err(e) => Ok((LV::Nil, Some(e.to_string()))),
                    }
                })?,
            )?;
            let f = find.clone();
            lua.globals().set(
                "dofile",
                lua.create_function(move |lua, p: String| {
                    let Some(path) = f(&p, true) else {
                        return Err(mlua::Error::runtime(format!("dofile: no skin file {p}")));
                    };
                    let src = crate::platform::vfs::read(&path)
                        .map_err(|e| mlua::Error::runtime(e.to_string()))?;
                    text_chunk(lua, &src, &path).call::<mlua::MultiValue>(())
                })?,
            )?;
            // io reads the skin's files; what scripts write goes nowhere
            let read = lua.create_function(move |lua, p: String| {
                let data = find(&p, false)
                    .filter(|p| crate::platform::vfs::stamp(p).1 <= IO_MAX_BYTES)
                    .and_then(|p| crate::platform::vfs::read(&p).ok());
                match data {
                    Some(d) => Ok(LV::String(lua.create_string(d)?)),
                    None => Ok(LV::Nil),
                }
            })?;
            lua.load(IO).set_name("vibebms io").call::<()>(read)?;
            lua.load(PRELUDE)
                .set_name("vibebms prelude")
                .call::<()>((vb, global))?;
            Ok(Script {
                lua,
                frame,
                fns: None,
            })
        }

        pub fn lua(&self) -> &Lua {
            &self.lua
        }

        /// Run `f` (the skin's own code) with `main_state` answering from
        /// `st`: skins read the scene's values while they are built.
        pub fn with_state<R>(
            &self,
            st: Option<&dyn SkinState>,
            f: impl FnOnce() -> mlua::Result<R>,
        ) -> mlua::Result<R> {
            let Some(st) = st else { return f() };
            let r = self.frame.with(st, f);
            // what scripts set while the skin is built is not kept
            self.frame.set_timers.borrow_mut().clear();
            self.frame.exec.borrow_mut().clear();
            r
        }

        /// Turn the evaluated skin table into JSON, keeping its functions.
        pub fn to_json(&mut self, v: LV) -> mlua::Result<Value> {
            use mlua::LuaSerdeExt;
            let fns: Table = self
                .lua
                .load(WALK)
                .set_name("vibebms walk")
                .call((v.clone(), FN_MARK))?;
            self.fns = Some(fns);
            let opts = mlua::serde::DeserializeOptions::new()
                .deny_unsupported_types(false)
                .deny_recursive_tables(false);
            self.lua.from_value_with::<Value>(v, opts)
        }
    }

    type StatePtr = *const (dyn SkinState + 'static);

    /// What the state functions (`main_state`) answer from: the scene's
    /// state while a frame runs (or a skin is built), and what scripts set.
    #[derive(Default)]
    struct Frame {
        st: Cell<Option<StatePtr>>,
        now_us: Cell<i64>,
        /// Timers set by scripts (`set_timer`, `customTimers`), µs.
        set_timers: RefCell<HashMap<i32, i64>>,
        /// Game events scripts asked for (`main_state.event_exec`).
        exec: RefCell<Vec<i32>>,
    }

    impl Frame {
        /// Run `f` with the state functions answering from `st`.
        fn with<R>(&self, st: &dyn SkinState, f: impl FnOnce() -> R) -> R {
            struct Restore<'a>(&'a Cell<Option<StatePtr>>, Option<StatePtr>);
            impl Drop for Restore<'_> {
                fn drop(&mut self) {
                    self.0.set(self.1);
                }
            }
            // SAFETY: only the lifetime is erased; the pointer is put back
            // (also on unwinding) before the borrow of `st` ends, and is
            // only read by `state` in the meantime
            let p = unsafe { std::mem::transmute::<*const (dyn SkinState + '_), StatePtr>(st) };
            let _restore = Restore(&self.st, self.st.replace(Some(p)));
            f()
        }

        /// The state of the `with` running now.
        fn state(&self) -> Option<&dyn SkinState> {
            // SAFETY: set only while `with` runs, whose `st` outlives every
            // call of the state functions made in it
            self.st.get().map(|p| unsafe { &*p })
        }
    }

    /// The state access functions `vb` holds. Outside a frame they are
    /// inert and take any arguments; `inert` is their answer then.
    fn install(lua: &Lua, vb: &Table, frame: &Rc<Frame>) -> mlua::Result<()> {
        fn put<A: mlua::FromLuaMulti, R: IntoLuaMulti>(
            lua: &Lua,
            vb: &Table,
            frame: &Rc<Frame>,
            name: &str,
            inert: fn(&Lua) -> mlua::Result<LV>,
            f: impl Fn(&Lua, &dyn SkinState, &Frame, A) -> mlua::Result<R> + 'static,
        ) -> mlua::Result<()> {
            let frame = frame.clone();
            let func =
                lua.create_function(move |lua, args: mlua::MultiValue| match frame.state() {
                    Some(st) => {
                        f(lua, st, &frame, A::from_lua_multi(args, lua)?)?.into_lua_multi(lua)
                    }
                    None => inert(lua)?.into_lua_multi(lua),
                })?;
            vb.set(name, func)
        }
        let zero: fn(&Lua) -> mlua::Result<LV> = |_| Ok(LV::Integer(0));
        let num = |st: &dyn SkinState, id: i32| st.number(id).unwrap_or(0);
        let notes = move |st: &dyn SkinState| (num(st, 74).max(1) * 2) as f64;
        put(lua, vb, frame, "number", zero, move |_, st, _, id: i32| {
            Ok(num(st, id))
        })?;
        put(lua, vb, frame, "float_number", zero, |_, st, _, id: i32| {
            Ok(st.rate(id) as f64)
        })?;
        let no: fn(&Lua) -> mlua::Result<LV> = |_| Ok(LV::Boolean(false));
        put(lua, vb, frame, "option", no, |_, st, _, id: i32| {
            Ok(st.option(id))
        })?;
        let empty: fn(&Lua) -> mlua::Result<LV> = |lua| lua.create_string("").map(LV::String);
        put(lua, vb, frame, "text", empty, |_, st, _, id: i32| {
            Ok(st.text(id).unwrap_or_default())
        })?;
        put(lua, vb, frame, "event_index", zero, |_, st, _, id: i32| {
            Ok(st.button(id).unwrap_or(0))
        })?;
        let off: fn(&Lua) -> mlua::Result<LV> = |_| Ok(LV::Integer(i64::MIN));
        put(lua, vb, frame, "timer", off, |_, st, fr, id: i32| {
            if let Some(us) = fr.set_timers.borrow().get(&id) {
                return Ok(*us);
            }
            Ok(st
                .timer(id)
                .map(|t| t.saturating_mul(1000))
                .unwrap_or(i64::MIN))
        })?;
        let fr = frame.clone();
        vb.set(
            "time",
            lua.create_function(move |_, ()| Ok(fr.now_us.get()))?,
        )?;
        let no_offset: fn(&Lua) -> mlua::Result<LV> = |lua| lua.create_table().map(LV::Table);
        put(
            lua,
            vb,
            frame,
            "offset",
            no_offset,
            |lua, st, _, id: i32| {
                let t = lua.create_table()?;
                let o = st.offset(id).unwrap_or_default();
                for (k, v) in ["x", "y", "w", "h", "r", "a"].iter().zip(o) {
                    t.set(*k, v)?;
                }
                Ok(t)
            },
        )?;
        put(
            lua,
            vb,
            frame,
            "set_timer",
            zero,
            |_, _, fr, (id, v): (i32, i64)| {
                fr.set_timers.borrow_mut().insert(id, v);
                Ok(true)
            },
        )?;
        put(lua, vb, frame, "event_exec", zero, |_, _, fr, id: i32| {
            fr.exec.borrow_mut().push(id);
            Ok(true)
        })?;
        put(lua, vb, frame, "exscore", zero, move |_, st, _, ()| {
            Ok(num(st, 71))
        })?;
        put(lua, vb, frame, "rate", zero, move |_, st, _, ()| {
            Ok(num(st, 71) as f64 / notes(st))
        })?;
        put(lua, vb, frame, "exscore_best", zero, move |_, st, _, ()| {
            Ok(num(st, 150))
        })?;
        put(lua, vb, frame, "rate_best", zero, move |_, st, _, ()| {
            Ok(num(st, 150) as f64 / notes(st))
        })?;
        // the rival: the target (a battle's opponent)
        put(
            lua,
            vb,
            frame,
            "exscore_rival",
            zero,
            move |_, st, _, ()| Ok(num(st, 121)),
        )?;
        put(lua, vb, frame, "rate_rival", zero, move |_, st, _, ()| {
            Ok(num(st, 121) as f64 / notes(st))
        })?;
        put(lua, vb, frame, "gauge", zero, move |_, st, _, ()| {
            Ok(num(st, 107))
        })?;
        put(lua, vb, frame, "judge", zero, move |_, st, _, j: i32| {
            Ok(match j {
                0..=4 => num(st, 110 + j),
                _ => num(st, 420),
            })
        })?;
        vb.set(
            "gauge_type",
            lua.create_function(|_, _: mlua::MultiValue| Ok(0))?,
        )
    }

    struct Entry {
        kind: ScriptKind,
        f: Function,
    }

    struct CustomEvent {
        action: Option<Function>,
        exec: i32,
        condition: Option<Function>,
        min_interval_ms: i64,
        last_us: i64,
    }

    /// Collects a skin's script values while it is parsed.
    #[derive(Default)]
    pub struct Builder {
        script: Option<Script>,
        home: Home,
        entries: Vec<Entry>,
        timers: Vec<(i32, Function)>,
        events: Vec<CustomEvent>,
    }

    impl Builder {
        /// `script` = the Lua state a Lua skin ran in (JSON skins get one
        /// when they first use a script string).
        pub fn new(script: Option<Script>, home: &Home) -> Builder {
            Builder {
                script,
                home: home.clone(),
                ..Default::default()
            }
        }

        fn lua(&mut self) -> Option<&Script> {
            if self.script.is_none() {
                match Script::new(&self.home, true) {
                    Ok(s) => self.script = Some(s),
                    Err(e) => {
                        log::warn!("skin script: {e}");
                        return None;
                    }
                }
            }
            self.script.as_ref()
        }

        /// The function a field holds: a Lua function of a Lua skin, or
        /// Lua source (an expression; statements for events).
        fn function(&mut self, v: &Value, kind: ScriptKind) -> Option<Function> {
            let s = v.as_str()?;
            if let Some(n) = s.strip_prefix(FN_MARK) {
                let n: i64 = n.parse().ok()?;
                return self.script.as_ref()?.fns.as_ref()?.get(n).ok();
            }
            if s.trim().is_empty() {
                return None;
            }
            let lua = self.lua()?.lua();
            let code = if kind == ScriptKind::Event {
                s.to_string()
            } else {
                format!("return {s}")
            };
            let f = match lua.load(code).set_name(s).into_function() {
                Ok(f) => f,
                Err(e) => {
                    log::warn!("skin script `{s}`: {e}");
                    return None;
                }
            };
            if kind == ScriptKind::Timer {
                // a script may produce the timer function itself
                if let Ok(LV::Function(t)) = f.call::<LV>(()) {
                    return Some(t);
                }
            }
            Some(f)
        }

        /// A field given as a script: its synthetic id.
        pub fn register(&mut self, v: &Value, kind: ScriptKind) -> Option<i32> {
            let f = self.function(v, kind)?;
            self.entries.push(Entry { kind, f });
            Some(SCRIPT_ID + self.entries.len() as i32 - 1)
        }

        /// `customTimers`: timer `id` follows a script.
        pub fn custom_timer(&mut self, id: i32, v: &Value) {
            if let Some(f) = self.function(v, ScriptKind::Timer) {
                self.timers.push((id, f));
            }
        }

        /// `customEvents`: `action` (script or event id) runs whenever
        /// `condition` holds, at most every `min_interval_ms`.
        pub fn custom_event(&mut self, action: &Value, condition: Option<&Value>, min_ms: i64) {
            let exec = action.as_i64().unwrap_or(0) as i32;
            let action = self.function(action, ScriptKind::Event);
            if action.is_none() && exec == 0 {
                return;
            }
            let condition = condition.and_then(|c| self.function(c, ScriptKind::Bool));
            self.events.push(CustomEvent {
                action,
                exec,
                condition,
                min_interval_ms: min_ms,
                last_us: i64::MIN,
            });
        }

        pub fn finish(self) -> Option<std::rc::Rc<Runtime>> {
            if self.entries.is_empty() && self.timers.is_empty() && self.events.is_empty() {
                return None;
            }
            let s = self.script?;
            let n = self.entries.len();
            Some(std::rc::Rc::new(Runtime {
                lua: s.lua,
                frame: s.frame,
                entries: self.entries,
                timers: self.timers,
                events: RefCell::new(self.events),
                cache: RefCell::new(vec![(0, Val::Nil); n]),
                generation: Cell::new(0),
                queued: RefCell::new(Vec::new()),
                failed: RefCell::new(HashSet::new()),
            }))
        }
    }

    /// A loaded skin's scripts.
    pub struct Runtime {
        lua: Lua,
        frame: Rc<Frame>,
        entries: Vec<Entry>,
        timers: Vec<(i32, Function)>,
        events: RefCell<Vec<CustomEvent>>,
        /// Values with the frame they are of: a value is worked out when
        /// first read in a frame, so what is not drawn does not run.
        cache: RefCell<Vec<(u64, Val)>>,
        /// The frame (counted by `update`).
        generation: Cell<u64>,
        /// Clicked script events, run on the next frame.
        queued: RefCell<Vec<usize>>,
        failed: RefCell<HashSet<usize>>,
    }

    impl std::fmt::Debug for Runtime {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "Runtime({} scripts)", self.entries.len())
        }
    }

    fn val(v: LV) -> Val {
        match v {
            LV::Boolean(b) => Val::Bool(b),
            LV::Integer(i) => Val::Int(i),
            LV::Number(n) => Val::Num(n),
            LV::String(s) => Val::Str(s.to_string_lossy()),
            LV::Nil => Val::Nil,
            _ => Val::Bool(true),
        }
    }

    impl Runtime {
        /// `f` of script `k`'s value this frame, `st` the state it reads.
        pub(super) fn value<R>(
            &self,
            k: usize,
            st: &dyn SkinState,
            f: impl FnOnce(&Val) -> R,
        ) -> R {
            let Some(e) = self.entries.get(k).filter(|e| e.kind != ScriptKind::Event) else {
                return f(&Val::Nil);
            };
            let now = self.generation.get();
            if self.cache.borrow()[k].0 != now {
                let v = self.frame.with(st, || self.eval(k, e));
                self.cache.borrow_mut()[k] = (now, v);
            }
            f(&self.cache.borrow()[k].1)
        }

        fn eval(&self, k: usize, e: &Entry) -> Val {
            match e.f.call::<LV>(()) {
                Ok(v) => val(v),
                Err(err) => {
                    self.warn(k, &err);
                    if e.kind == ScriptKind::Timer {
                        Val::Int(i64::MIN)
                    } else {
                        Val::Nil
                    }
                }
            }
        }

        /// A timer scripts control: `Some(None)` = off.
        pub(super) fn timer_override(&self, id: i32) -> Option<Option<i64>> {
            self.frame
                .set_timers
                .borrow()
                .get(&id)
                .map(|us| (*us != i64::MIN).then(|| us.div_euclid(1000)))
        }

        /// A click on an object whose `act` is `id`: script events run on
        /// the next frame. `false` if `id` is not a script.
        pub fn click(&self, id: i32) -> bool {
            match script_index(id) {
                Some(k) if k < self.entries.len() => {
                    self.queued.borrow_mut().push(k);
                    true
                }
                _ => false,
            }
        }

        /// Game events requested by scripts since the last call.
        pub fn take_events(&self) -> Vec<i32> {
            std::mem::take(&mut *self.frame.exec.borrow_mut())
        }

        fn warn(&self, k: usize, e: &mlua::Error) {
            if self.failed.borrow_mut().insert(k) {
                log::warn!("skin script #{k}: {e}");
            }
        }

        /// Start a frame (`now` = scene ms): run clicked and custom events,
        /// custom timers and timer scripts (they watch for changes, so they
        /// run every frame); other values are worked out when read.
        pub fn update(&self, st: &dyn SkinState, now: i64) {
            arm(&self.lua, FRAME_TIME);
            let now_us = now.saturating_mul(1000);
            self.frame.now_us.set(now_us);
            self.generation.set(self.generation.get() + 1);
            self.frame.with(st, || {
                let queued = std::mem::take(&mut *self.queued.borrow_mut());
                for k in queued {
                    if let Err(e) = self.entries[k].f.call::<()>(()) {
                        self.warn(k, &e);
                    }
                }
                for ev in self.events.borrow_mut().iter_mut() {
                    let on = match &ev.condition {
                        Some(c) => c.call::<LV>(()).map(|v| val(v).truthy()).unwrap_or(false),
                        None => true,
                    };
                    let due = ev.last_us == i64::MIN
                        || (now_us - ev.last_us) / 1000 >= ev.min_interval_ms;
                    if on && due {
                        ev.last_us = now_us;
                        match &ev.action {
                            Some(a) => {
                                if let Err(e) = a.call::<()>(()) {
                                    log::debug!("skin custom event: {e}");
                                }
                            }
                            None => self.frame.exec.borrow_mut().push(ev.exec),
                        }
                    }
                }
                for (id, f) in &self.timers {
                    let v = f.call::<LV>(()).map(|v| val(v).int()).ok().flatten();
                    self.frame
                        .set_timers
                        .borrow_mut()
                        .insert(*id, v.unwrap_or(i64::MIN));
                }
                let now = self.generation.get();
                for (k, e) in self.entries.iter().enumerate() {
                    if e.kind == ScriptKind::Timer {
                        let v = self.eval(k, e);
                        self.cache.borrow_mut()[k] = (now, v);
                    }
                }
            });
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// The home of a skin in `dir`, a folder of its theme.
        fn home(dir: &Path) -> Home {
            Home::of(
                &dir.join("s.luaskin"),
                dir.parent().unwrap().parent().unwrap(),
            )
        }

        fn eval<T: mlua::FromLuaMulti>(code: &str) -> mlua::Result<T> {
            let s = Script::new(&home(Path::new("/nonexistent/theme/skin")), true).unwrap();
            s.lua().load(code).eval::<T>()
        }

        #[test]
        fn string_patterns_as_lua() {
            let v: String = eval(
                r#"local a, b = ("hello world"):find("o w")
                local t = {}
                for w in ("one two  three"):gmatch("%a+") do t[#t + 1] = w end
                local g, n = ("abc"):gsub("%w", { a = "A", c = false })
                local f = string.gsub("x=1, y=22", "(%w+)=(%w+)", function(k, v) return v .. k end)
                return table.concat({ a, b, table.concat(t, ","), g, n, f,
                  string.match("  trim  ", "^%s*(.-)%s*$"), ("x"):rep(3):find("xx", 2, true),
                  string.format("%d|%s", 3.0, "s"), tostring(("ab"):match("()b")) }, ";")"#,
            )
            .unwrap();
            assert_eq!(v, "5;7;one,two,three;Abc;3;1x, 22y;trim;2;3|s;2");
        }

        #[test]
        fn http_answers_as_beatoraja_with_the_network_off() {
            let v: String = eval(
                r#"local ms = require("main_state")
                local id, e = ms.http_get_async("https://example.com")
                local p = ms.http_poll(1)
                local s, e2 = http_get("https://example.com")
                return table.concat({ tostring(id), e, p.status, p.status_code, p.error, #p.lines,
                  tostring(ms.http_cancel(1)), tostring(s), e2, tostring(ms.open_url("x")) }, ";")"#,
            )
            .unwrap();
            assert_eq!(
                v,
                "nil;HTTP disabled;failed;0;HTTP disabled;0;false;nil;HTTP disabled;false"
            );
        }

        #[cfg(unix)]
        #[test]
        fn a_link_out_of_the_skin_folder_is_not_followed() {
            let dir = std::env::temp_dir().join(format!("vibebms-lua-link-{}", std::process::id()));
            let skin = dir.join("theme").join("skin");
            std::fs::create_dir_all(&skin).unwrap();
            std::fs::write(dir.join("secret.txt"), "secret").unwrap();
            std::fs::write(skin.join("own.txt"), "own").unwrap();
            std::os::unix::fs::symlink(dir.join("secret.txt"), skin.join("link.txt")).unwrap();
            let s = Script::new(&home(&skin), true).unwrap();
            let read = |name: &str| -> Option<String> {
                let code = format!(r#"local f = io.open("{name}") return f and f:read("a")"#);
                s.lua().load(code).eval().unwrap()
            };
            assert_eq!(read("own.txt").as_deref(), Some("own"));
            assert_eq!(read("link.txt"), None);
            std::fs::remove_dir_all(&dir).ok();
        }

        struct St;
        impl SkinState for St {
            fn timer(&self, id: i32) -> Option<i64> {
                (id == 41).then_some(2)
            }
            fn option(&self, id: i32) -> bool {
                id == 1
            }
            fn number(&self, id: i32) -> Option<i64> {
                Some(id as i64 * 10)
            }
            fn text(&self, _: i32) -> Option<String> {
                Some("t".into())
            }
            fn rate(&self, _: i32) -> f32 {
                0.5
            }
        }

        #[test]
        fn state_functions_answer_from_the_state_only_while_it_is_given() {
            let s = Script::new(&home(Path::new("/nonexistent/theme/skin")), true).unwrap();
            type Seen = (i64, bool, String, i64, Option<i64>, LV);
            let read = || -> mlua::Result<Seen> {
                let code =
                    "return number(7), option(1), text(1), timer(41), offset(3).x, set_timer(5, 9)";
                s.lua().load(code).eval()
            };
            let inert = (0, false, String::new(), i64::MIN, None, LV::Integer(0));
            assert_eq!(read().unwrap(), inert);
            let seen = s.with_state(Some(&St), read).unwrap();
            assert_eq!(
                seen,
                (70, true, "t".into(), 2000, Some(0), LV::Boolean(true))
            );
            assert_eq!(read().unwrap(), inert);
            // a timer set while the skin was built is not kept
            let t = s.with_state(Some(&St), || s.lua().load("return timer(5)").eval::<i64>());
            assert_eq!(t.unwrap(), i64::MIN);
        }

        #[test]
        fn values_run_when_read_and_timers_every_frame() {
            let dir = Path::new("/nonexistent/theme/skin");
            let s = Script::new(&home(dir), true).unwrap();
            s.lua()
                .load("calls, ticks = 0, 0 function count() calls = calls + 1 return calls end")
                .exec()
                .unwrap();
            let mut b = Builder::new(Some(s), &home(dir));
            let s = |code: &str| Value::String(code.into());
            let counted = b.register(&s("count()"), ScriptKind::Int).unwrap();
            let timer = b.register(&s("timer(900)"), ScriptKind::Int).unwrap();
            b.register(
                &s("(function() ticks = ticks + 1 return 0 end)()"),
                ScriptKind::Timer,
            );
            b.custom_timer(900, &s("1234000"));
            let rt = b.finish().unwrap();
            let offsets = HashMap::new();
            let ov = Overlay {
                st: &St,
                rt: Some(&rt),
                offsets: &offsets,
            };
            let global = |k: &str| rt.lua.globals().get::<i64>(k).unwrap();
            // (a timer script runs once as it is read, in case it makes the
            // timer function)
            let ticks = global("ticks");
            rt.update(&St, 0);
            rt.update(&St, 16);
            // not read: not run; timers run every frame
            assert_eq!((global("calls"), global("ticks")), (0, ticks + 2));
            assert_eq!(ov.number(counted), Some(1));
            assert_eq!(ov.number(counted), Some(1));
            // what custom timers set this frame is seen by the values
            assert_eq!(ov.number(timer), Some(1234000));
            rt.update(&St, 32);
            assert_eq!(ov.number(counted), Some(2));
            assert_eq!(global("ticks"), ticks + 3);
        }

        #[test]
        fn a_runaway_pattern_is_stopped() {
            let r: mlua::Result<LV> = eval(r#"return ("a"):rep(5000):find(".-.-.-.-.-x")"#);
            assert!(r.unwrap_err().to_string().contains("too long"));
        }

        #[test]
        fn coroutines_and_pcall_do_not_outlive_the_time() {
            let t = std::time::Instant::now();
            for code in [
                "local co = coroutine.create(function() while true do end end) coroutine.resume(co)",
                "coroutine.wrap(function() while true do end end)()",
                "while true do pcall(function() for i = 1, 1e7 do end end) end",
                "while true do coroutine.resume(coroutine.create(function() for i = 1, 1e7 do end end)) end",
            ] {
                let r: mlua::Result<()> = eval(code);
                assert!(r.unwrap_err().to_string().contains("too long"), "{code}");
            }
            assert!(t.elapsed() < std::time::Duration::from_secs(20));
        }
    }
}
