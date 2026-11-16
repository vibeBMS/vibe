//! beatoraja skin scripts: values given as Lua functions (Lua skins) or Lua
//! source strings (JSON skins) — draw conditions, timers, numbers, texts,
//! image indices, slider / graph rates and click events.
//!
//! Each script value gets a synthetic id (`SCRIPT_ID + n`) in the namespace
//! of what it replaces (option, timer, number, ...). Once per frame
//! [`Runtime::update`] evaluates every script against the scene's
//! [`SkinState`], and [`Overlay`] answers those ids from the results.
//! Scripts reach the game through beatoraja's `main_state`, `timer_util`
//! and `event_util` modules (as globals in JSON skins).

use super::SkinState;
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
    fn val(&self, id: i32) -> Option<Val> {
        let rt = self.rt?;
        script_index(id).map(|k| rt.value(k))
    }
}

impl SkinState for Overlay<'_> {
    fn timer(&self, id: i32) -> Option<i64> {
        if let Some(us) = self.rt.and_then(|rt| rt.timer_override(id)) {
            return us;
        }
        match self.val(id) {
            Some(v) => match v.int() {
                Some(us) if us != i64::MIN => Some(us.div_euclid(1000)),
                _ => None,
            },
            None => self.st.timer(id),
        }
    }
    fn option(&self, id: i32) -> bool {
        match self.val(id.abs()) {
            Some(v) => v.truthy() == (id > 0),
            None => self.st.option(id),
        }
    }
    fn number(&self, id: i32) -> Option<i64> {
        match self.val(id) {
            Some(v) => v.int(),
            None => self.st.number(id),
        }
    }
    fn text(&self, id: i32) -> Option<String> {
        match self.val(id) {
            Some(v) => Some(v.text()),
            None => self.st.text(id),
        }
    }
    fn rate(&self, id: i32) -> f32 {
        match self.val(id) {
            Some(v) => v.num() as f32,
            None => self.st.rate(id),
        }
    }
    fn button(&self, id: i32) -> Option<i64> {
        match self.val(id) {
            Some(v) => v.int(),
            None => self.st.button(id),
        }
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

#[cfg(not(target_arch = "wasm32"))]
pub use native::*;

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;
    use mlua::{Function, Lua, Table, Value as LV};
    use serde_json::Value;
    use std::cell::RefCell;
    use std::collections::{HashMap, HashSet};
    use std::path::Path;

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
-- anything else (input, audio, volume, http ...) is a harmless no-op
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
        let name = name.replace('\\', "/");
        let p = Path::new(&name);
        let mut cands = Vec::new();
        cands.push(p.to_path_buf());
        if !p.is_absolute() {
            let tail = name
                .get(..5)
                .filter(|h| h.eq_ignore_ascii_case("skin/"))
                .and_then(|_| name[5..].split_once('/'))
                .map(|(_, t)| t);
            for a in dir.ancestors() {
                cands.push(a.join(p));
                if let Some(t) = tail {
                    cands.push(a.join(t));
                }
                if crate::platform::vfs::normalize(a) == *root {
                    break;
                }
            }
        }
        cands
            .into_iter()
            .map(|c| crate::platform::vfs::resolve_case(&crate::platform::vfs::normalize(&c)))
            .filter(|c| c.starts_with(root))
            .filter(|c| {
                !lua_only
                    || c.extension().is_some_and(|e| {
                        e.eq_ignore_ascii_case("lua") || e.eq_ignore_ascii_case("luaskin")
                    })
            })
            .find(|c| crate::platform::vfs::is_file(c))
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
    struct Deadline(std::time::Instant);

    fn arm(lua: &Lua, budget: std::time::Duration) {
        lua.set_app_data(Deadline(std::time::Instant::now() + budget));
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
        vb: Table,
        fns: Option<Table>,
    }

    impl Script {
        /// A Lua state with the skin modules; `global` also exposes them as
        /// globals (JSON skins), `dir` is the skin's `require` path.
        pub fn new(dir: &Path, global: bool) -> mlua::Result<Script> {
            // no debug, and os / io as beatoraja restricts them (below):
            // scripts only reach what the skin modules give
            let libs = mlua::StdLib::COROUTINE
                | mlua::StdLib::TABLE
                | mlua::StdLib::STRING
                | mlua::StdLib::UTF8
                | mlua::StdLib::MATH
                | mlua::StdLib::OS
                | mlua::StdLib::PACKAGE;
            let lua = Lua::new_with(libs, mlua::LuaOptions::default())?;
            lua.set_memory_limit(MEMORY_LIMIT)?;
            lua.set_hook(
                mlua::HookTriggers::new().every_nth_instruction(10_000),
                |lua, _| match lua.app_data_ref::<Deadline>() {
                    Some(d) if std::time::Instant::now() > d.0 => {
                        Err(mlua::Error::runtime("skin script ran too long"))
                    }
                    _ => Ok(mlua::VmState::Continue),
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
            let vb = lua.create_table()?;
            defaults(&lua, &vb, 0)?;
            // `require`, `dofile` and `io` read files of the skin's theme
            // folder (the parent of the skin file's folder) only
            let root = crate::platform::vfs::normalize(dir.parent().unwrap_or(dir));
            let base = dir.to_path_buf();
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
                .call::<()>((vb.clone(), global))?;
            Ok(Script { lua, vb, fns: None })
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
            let timers = RefCell::new(HashMap::new());
            let exec = RefCell::new(Vec::new());
            let r = self.lua.scope(|s| {
                bind(s, &self.vb, st, &timers, &exec, 0)?;
                f()
            });
            let _ = defaults(&self.lua, &self.vb, 0);
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

    /// State access functions before / outside a frame.
    fn defaults(lua: &Lua, vb: &Table, now_us: i64) -> mlua::Result<()> {
        let zero = lua.create_function(|_, _: mlua::MultiValue| Ok(0))?;
        for k in [
            "number",
            "float_number",
            "event_index",
            "rate",
            "exscore",
            "rate_best",
            "exscore_best",
            "rate_rival",
            "exscore_rival",
            "gauge",
            "gauge_type",
            "judge",
            "set_timer",
            "event_exec",
        ] {
            vb.set(k, zero.clone())?;
        }
        vb.set(
            "option",
            lua.create_function(|_, _: mlua::MultiValue| Ok(false))?,
        )?;
        vb.set(
            "text",
            lua.create_function(|_, _: mlua::MultiValue| Ok(""))?,
        )?;
        vb.set(
            "offset",
            lua.create_function(|lua, _: mlua::MultiValue| lua.create_table())?,
        )?;
        vb.set(
            "timer",
            lua.create_function(|_, _: mlua::MultiValue| Ok(i64::MIN))?,
        )?;
        vb.set("time", lua.create_function(move |_, ()| Ok(now_us))?)?;
        Ok(())
    }

    /// The state access functions `vb` holds while a scope lasts.
    fn bind<'scope, 'env: 'scope>(
        s: &'scope mlua::Scope<'scope, 'env>,
        vb: &Table,
        st: &'scope dyn SkinState,
        set_timers: &'scope RefCell<HashMap<i32, i64>>,
        exec: &'scope RefCell<Vec<i32>>,
        now_us: i64,
    ) -> mlua::Result<()> {
        let timer = move |id: i32| -> i64 {
            if let Some(us) = set_timers.borrow().get(&id) {
                return *us;
            }
            st.timer(id)
                .map(|t| t.saturating_mul(1000))
                .unwrap_or(i64::MIN)
        };
        vb.set(
            "number",
            s.create_function(move |_, id: i32| Ok(st.number(id).unwrap_or(0)))?,
        )?;
        vb.set(
            "float_number",
            s.create_function(move |_, id: i32| Ok(st.rate(id) as f64))?,
        )?;
        vb.set(
            "option",
            s.create_function(move |_, id: i32| Ok(st.option(id)))?,
        )?;
        vb.set(
            "text",
            s.create_function(move |_, id: i32| Ok(st.text(id).unwrap_or_default()))?,
        )?;
        vb.set(
            "event_index",
            s.create_function(move |_, id: i32| Ok(st.button(id).unwrap_or(0)))?,
        )?;
        vb.set("timer", s.create_function(move |_, id: i32| Ok(timer(id)))?)?;
        vb.set("time", s.create_function(move |_, ()| Ok(now_us))?)?;
        vb.set(
            "offset",
            s.create_function(move |lua, id: i32| {
                let t = lua.create_table()?;
                let o = st.offset(id).unwrap_or_default();
                for (k, v) in ["x", "y", "w", "h", "r", "a"].iter().zip(o) {
                    t.set(*k, v)?;
                }
                Ok(t)
            })?,
        )?;
        vb.set(
            "set_timer",
            s.create_function(move |_, (id, v): (i32, i64)| {
                set_timers.borrow_mut().insert(id, v);
                Ok(true)
            })?,
        )?;
        vb.set(
            "event_exec",
            s.create_function(move |_, id: i32| {
                exec.borrow_mut().push(id);
                Ok(true)
            })?,
        )?;
        let num = move |id: i32| st.number(id).unwrap_or(0);
        let notes = move || (num(74).max(1) * 2) as f64;
        vb.set("exscore", s.create_function(move |_, ()| Ok(num(71)))?)?;
        vb.set(
            "rate",
            s.create_function(move |_, ()| Ok(num(71) as f64 / notes()))?,
        )?;
        vb.set(
            "exscore_best",
            s.create_function(move |_, ()| Ok(num(150)))?,
        )?;
        vb.set(
            "rate_best",
            s.create_function(move |_, ()| Ok(num(150) as f64 / notes()))?,
        )?;
        vb.set("gauge", s.create_function(move |_, ()| Ok(num(107)))?)?;
        vb.set(
            "judge",
            s.create_function(move |_, j: i32| {
                Ok(match j {
                    0..=4 => num(110 + j),
                    _ => num(420),
                })
            })?,
        )?;
        Ok(())
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
        dir: std::path::PathBuf,
        entries: Vec<Entry>,
        timers: Vec<(i32, Function)>,
        events: Vec<CustomEvent>,
    }

    impl Builder {
        /// `script` = the Lua state a Lua skin ran in (JSON skins get one
        /// when they first use a script string).
        pub fn new(script: Option<Script>, dir: &Path) -> Builder {
            Builder {
                script,
                dir: dir.to_path_buf(),
                ..Default::default()
            }
        }

        fn lua(&mut self) -> Option<&Script> {
            if self.script.is_none() {
                match Script::new(&self.dir, true) {
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
                vb: s.vb,
                entries: self.entries,
                timers: self.timers,
                events: RefCell::new(self.events),
                cache: RefCell::new(vec![Val::Nil; n]),
                set_timers: RefCell::new(HashMap::new()),
                queued: RefCell::new(Vec::new()),
                exec: RefCell::new(Vec::new()),
                failed: RefCell::new(HashSet::new()),
            }))
        }
    }

    /// A loaded skin's scripts.
    pub struct Runtime {
        lua: Lua,
        vb: Table,
        entries: Vec<Entry>,
        timers: Vec<(i32, Function)>,
        events: RefCell<Vec<CustomEvent>>,
        cache: RefCell<Vec<Val>>,
        /// Timers set by scripts (`set_timer`, `customTimers`), µs.
        set_timers: RefCell<HashMap<i32, i64>>,
        /// Clicked script events, run on the next frame.
        queued: RefCell<Vec<usize>>,
        /// Game events scripts asked for (`main_state.event_exec`).
        exec: RefCell<Vec<i32>>,
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
        pub(super) fn value(&self, k: usize) -> Val {
            self.cache.borrow().get(k).cloned().unwrap_or_default()
        }

        /// A timer scripts control: `Some(None)` = off.
        pub(super) fn timer_override(&self, id: i32) -> Option<Option<i64>> {
            self.set_timers
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
            std::mem::take(&mut *self.exec.borrow_mut())
        }

        fn warn(&self, k: usize, e: &mlua::Error) {
            if self.failed.borrow_mut().insert(k) {
                log::warn!("skin script #{k}: {e}");
            }
        }

        /// Evaluate every script for this frame (`now` = scene ms).
        pub fn update(&self, st: &dyn SkinState, now: i64) {
            arm(&self.lua, FRAME_TIME);
            let now_us = now.saturating_mul(1000);
            let r = self.lua.scope(|s| {
                bind(s, &self.vb, st, &self.set_timers, &self.exec, now_us)?;
                // clicked events, then custom events and timers, then values
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
                            None => self.exec.borrow_mut().push(ev.exec),
                        }
                    }
                }
                for (id, f) in &self.timers {
                    let v = f.call::<LV>(()).map(|v| val(v).int()).ok().flatten();
                    self.set_timers
                        .borrow_mut()
                        .insert(*id, v.unwrap_or(i64::MIN));
                }
                let mut cache = self.cache.borrow_mut();
                for (k, e) in self.entries.iter().enumerate() {
                    if e.kind == ScriptKind::Event {
                        continue;
                    }
                    cache[k] = match e.f.call::<LV>(()) {
                        Ok(v) => val(v),
                        Err(err) => {
                            self.warn(k, &err);
                            if e.kind == ScriptKind::Timer {
                                Val::Int(i64::MIN)
                            } else {
                                Val::Nil
                            }
                        }
                    };
                }
                Ok(())
            });
            if let Err(e) = r {
                log::warn!("skin scripts: {e}");
            }
            // the scoped functions are gone: back to inert ones
            let _ = defaults(&self.lua, &self.vb, now_us);
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub use web::*;

/// No Lua in the web build: script values keep their static fallbacks.
#[cfg(target_arch = "wasm32")]
mod web {
    use super::*;
    use serde_json::Value;
    use std::path::Path;

    pub struct Script;

    #[derive(Default)]
    pub struct Builder;

    impl Builder {
        pub fn new(_: Option<Script>, _: &Path) -> Builder {
            Builder
        }
        pub fn register(&mut self, _: &Value, _: ScriptKind) -> Option<i32> {
            None
        }
        pub fn custom_timer(&mut self, _: i32, _: &Value) {}
        pub fn custom_event(&mut self, _: &Value, _: Option<&Value>, _: i64) {}
        pub fn finish(self) -> Option<std::rc::Rc<Runtime>> {
            None
        }
    }

    #[derive(Debug)]
    pub struct Runtime;

    impl Runtime {
        pub(super) fn value(&self, _: usize) -> Val {
            Val::Nil
        }
        pub(super) fn timer_override(&self, _: i32) -> Option<Option<i64>> {
            None
        }
        pub fn click(&self, _: i32) -> bool {
            false
        }
        pub fn take_events(&self) -> Vec<i32> {
            Vec::new()
        }
        pub fn update(&self, _: &dyn SkinState, _: i64) {}
    }
}
