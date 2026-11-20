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
    use std::time::Duration;

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

    /// The file a skin script names (`require`, `dofile`, `io`), inside
    /// `root` (the skins folder). Paths are relative to the skin's folder,
    /// to one above it up to `root`, or
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
            // a name that leads out of `root` is dropped before anything
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
    /// Time a skin may take to load.
    #[cfg(not(test))]
    pub const LOAD_TIME: Duration = Duration::from_secs(10);
    #[cfg(test)]
    pub const LOAD_TIME: Duration = Duration::from_secs(3);
    /// Time a Lua skin may take to say what it is (its type, name and
    /// options: it runs for them when skins are listed, before it is
    /// chosen).
    #[cfg(not(test))]
    pub const HEADER_TIME: Duration = Duration::from_secs(2);
    #[cfg(test)]
    pub const HEADER_TIME: Duration = Duration::from_millis(100);
    /// Time a skin's scripts may take per frame.
    const FRAME_TIME: Duration = Duration::from_secs(1);
    /// Frames in a row a skin's scripts may run out of time before they
    /// are stopped for good.
    const LATE_FRAMES: u32 = 3;
    /// Lua instructions between two looks at the clock.
    const HOOK_EVERY: u32 = 1000;
    /// Timers a skin's scripts may set (kept outside Lua's memory limit).
    const SET_TIMERS: usize = 4096;
    /// Game events a click's script may ask for.
    const CLICK_EVENTS: usize = 8;
    /// Bytes of a text a script gives that are shown.
    const TEXT_BYTES: usize = 4096;

    /// When running scripts are stopped.
    struct Deadline(web_time::Instant);

    const TOO_LONG: &str = "skin script ran too long";

    fn expired(lua: &Lua) -> bool {
        lua.app_data_ref::<Deadline>()
            .is_some_and(|d| web_time::Instant::now() > d.0)
    }

    /// The error that stops a script past its time, for what the library
    /// does in Rust (a call of it is one instruction to the hook).
    fn in_time(lua: &Lua) -> mlua::Result<()> {
        match expired(lua) {
            true => Err(mlua::Error::runtime(TOO_LONG)),
            false => Ok(()),
        }
    }

    /// What lets a script outlive its time, closed (the hook that stops
    /// it counts Lua instructions, and only while hooks are allowed):
    /// - what catches errors (`pcall`, `xpcall`, `coroutine.resume` /
    ///   `wrap` / `close`, `load` of a reader function) passes that error
    ///   on, and a message handler is not called for it (it would run
    ///   with the hook off);
    /// - no finalizers (`__gc` runs with the hook off; beatoraja's Lua
    ///   has none either) and no garbage collection on demand;
    /// - a library function is one instruction however long it works: the
    ///   time is looked at after each one whose work grows with its
    ///   arguments, and the loops of the table library run in C only over
    ///   a plain table of at most `MAX` entries (in Lua otherwise: a
    ///   length or an element a metamethod makes up, a range of
    ///   millions), `string.rep` not at all for an empty result.
    ///
    /// `load` takes text chunks only (Lua does not verify compiled
    /// bytecode), and `string.dump` is gone.
    const GUARD: &str = r##"
local expired, too_long = ...
local pcall0, xpcall0, load0, error, type, select = pcall, xpcall, load, error, type, select
local getmetatable0, setmetatable0, rawget, rawset = getmetatable, setmetatable, rawget, rawset
local create, resume0, close0 = coroutine.create, coroutine.resume, coroutine.close
local tointeger, tonumber, maxinteger = math.tointeger, tonumber, math.maxinteger
local function check(...)
  if expired() then error(too_long, 0) end
  return ...
end
pcall = function(...) return check(pcall0(...)) end
xpcall = function(f, h, ...)
  if type(h) ~= "function" then return xpcall0(f, h, ...) end
  return check(xpcall0(f, function(e)
    if expired() then return e end
    return h(e)
  end, ...))
end
coroutine.resume = function(...) return check(resume0(...)) end
coroutine.close = function(...) return check(close0(...)) end
coroutine.wrap = function(f)
  local co = create(f)
  return function(...)
    local r = table.pack(check(resume0(co, ...)))
    if r[1] then return table.unpack(r, 2, r.n) end
    error(r[2], 0)
  end
end
load = function(chunk, name, _, ...)
  if select("#", ...) > 0 then return check(load0(chunk, name, "t", ...)) end
  return check(load0(chunk, name, "t"))
end
string.dump = nil

setmetatable = function(t, mt)
  local gc = nil
  if type(mt) == "table" then gc = rawget(mt, "__gc") end
  if gc == nil then return setmetatable0(t, mt) end
  -- the table is given its metatable while that has no `__gc`: Lua
  -- only finalizes what had one then
  rawset(mt, "__gc", nil)
  local ok, r = pcall0(setmetatable0, t, mt)
  rawset(mt, "__gc", gc)
  if not ok then error(r, 2) end
  return r
end
local count = collectgarbage
collectgarbage = function(opt)
  if opt == "count" then return count("count") end
  return 0
end

for lib, names in pairs({
  [string] = { "byte", "char", "find", "format", "gmatch", "gsub", "lower", "match", "pack",
    "packsize", "rep", "reverse", "sub", "unpack", "upper" },
  [table] = { "concat", "insert", "move", "remove", "sort", "unpack" },
  [utf8] = { "char", "codepoint", "len", "offset" },
  [os] = { "date" },
  [_G] = { "print", "warn" },
}) do
  for _, k in ipairs(names) do
    local f = lib[k]
    lib[k] = function(...) return check(f(...)) end
  end
end

local rep = string.rep
string.rep = function(s, n, sep)
  local k = tonumber(n)
  if k and k > 1 and s == "" and (sep == nil or sep == "") then n = 1 end
  return rep(s, n, sep)
end

local MAX = 1 << 20
local insert, remove, move, concat, sort = table.insert, table.remove, table.move, table.concat, table.sort
-- the C function is quick whatever it does with `t`: a plain table of at
-- most MAX entries, or no table and nothing with metamethods (an error)
local function quick(t)
  if getmetatable0(t) ~= nil then return false end
  return type(t) ~= "table" or #t <= MAX
end
local function len(t)
  local n = tointeger(#t)
  if not n then error("object length is not an integer", 3) end
  return n
end
table.insert = function(t, ...)
  if quick(t) then return insert(t, ...) end
  local n, e = select("#", ...), len(t) + 1
  if n == 1 then
    t[e] = (...)
  elseif n == 2 then
    local pos, v = ...
    pos = tointeger(pos)
    if not pos or pos < 1 or pos > e then
      error("bad argument #2 to 'insert' (position out of bounds)", 2)
    end
    for i = e, pos + 1, -1 do t[i] = t[i - 1] end
    t[pos] = v
  else
    error("wrong number of arguments to 'insert'", 2)
  end
end
table.remove = function(t, pos)
  if quick(t) then return remove(t, pos) end
  local size = len(t)
  if pos == nil then
    pos = size
  else
    pos = tointeger(pos)
    if not pos or pos ~= size and (pos < 1 or pos - 1 > size) then
      error("bad argument #2 to 'remove' (position out of bounds)", 2)
    end
  end
  local v = t[pos]
  if pos < size then
    for i = pos, size - 1 do t[i] = t[i + 1] end
    pos = size
  end
  t[pos] = nil
  return v
end
table.move = function(a1, f, e, t, a2)
  local fi, ei, ti = tointeger(f), tointeger(e), tointeger(t)
  -- (a wrong argument: the C function says so)
  if not (fi and ei and ti) then return move(a1, f, e, t, a2) end
  if ei < fi or quick(a1) and (a2 == nil or quick(a2)) and ei - fi < MAX then
    return move(a1, fi, ei, ti, a2)
  end
  if fi <= 0 and ei >= maxinteger + fi then
    error("bad argument #3 to 'move' (too many elements to move)", 2)
  end
  local n = ei - fi + 1
  if ti > maxinteger - n + 1 then
    error("bad argument #4 to 'move' (destination wrap around)", 2)
  end
  local to = a2 == nil and a1 or a2
  if ti > ei or ti <= fi or to ~= a1 then
    for i = 0, n - 1 do to[ti + i] = a1[fi + i] end
  else
    for i = n - 1, 0, -1 do to[ti + i] = a1[fi + i] end
  end
  return to
end
table.concat = function(t, sep, i, j)
  if quick(t) then return concat(t, sep, i, j) end
  local size = len(t)
  local first, last = i == nil and 1 or tointeger(i), j == nil and size or tointeger(j)
  if not (first and last) then return concat(t, sep, i, j) end
  local copy = {}
  for k = first, last do
    local v = t[k]
    -- (what is no string: the C function's error)
    if v == nil then break end
    copy[k] = v
  end
  return concat(copy, sep, first, last)
end
table.sort = function(t, cmp)
  if quick(t) then return sort(t, cmp) end
  local n = len(t)
  -- (more than a skin sorts)
  if n > MAX then error("bad argument #1 to 'sort' (array too big)", 2) end
  local copy = {}
  for k = 1, n do copy[k] = t[k] end
  sort(copy, cmp)
  for k = 1, n do t[k] = copy[k] end
end
"##;

    /// `string.find`, `match`, `gmatch` and `gsub` by `luapat` (a step
    /// budget per call; Lua's own run in C where the hook cannot stop them).
    /// The strings stay Lua's (within its memory limit) while they are
    /// matched.
    fn install_patterns(lua: &Lua) -> mlua::Result<()> {
        use super::super::luapat::{self, Cap};
        fn text(lua: &Lua, v: LV, n: usize, f: &str) -> mlua::Result<mlua::LuaString> {
            let tn = v.type_name();
            lua.coerce_string(v)?.ok_or_else(|| {
                mlua::Error::runtime(format!(
                    "bad argument #{n} to '{f}' (string expected, got {tn})"
                ))
            })
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
                    let (s, p) = (s.as_bytes(), p.as_bytes());
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
            let pos = luapat::gmatch_start(init.unwrap_or(1), s.as_bytes().len());
            let state = RefCell::new((pos, None));
            lua.create_function(move |lua, ()| {
                // (a loop over it is a few instructions a match)
                in_time(lua)?;
                let mut st = state.borrow_mut();
                let (pos, last) = &mut *st;
                let (s, p) = (s.as_bytes(), p.as_bytes());
                match luapat::gmatch_next(&s, &p, pos, last).map_err(err)? {
                    Some(c) => Ok(mlua::MultiValue::from_iter(caps(lua, &c)?)),
                    None => Ok(mlua::MultiValue::from_iter([LV::Nil])),
                }
            })
        })?;
        string.set("gmatch", gmatch)?;
        let gsub = lua.create_function(|lua, (s, p, repl, n): (LV, LV, LV, Option<i64>)| {
            let (s, p) = (text(lua, s, 1, "gsub")?, text(lua, p, 2, "gsub")?);
            let (s, p) = (s.as_bytes(), p.as_bytes());
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
                    let r = r.as_bytes();
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

    fn arm(lua: &Lua, budget: Duration) {
        lua.set_app_data(Deadline(web_time::Instant::now() + budget));
    }

    /// A Lua source file as a chunk (never compiled bytecode).
    pub fn text_chunk<'a>(
        lua: &'a Lua,
        src: impl mlua::chunk::AsChunk + 'a,
        path: &Path,
    ) -> mlua::chunk::Chunk<'a> {
        lua.load(src)
            .set_name(path.display().to_string())
            .set_mode(mlua::chunk::ChunkMode::Text)
    }

    /// The chunk of a Lua file a script names (`require`, `dofile`,
    /// `loadfile`: reading and compiling it is one instruction to the
    /// hook).
    fn source<'a>(lua: &'a Lua, path: &Path) -> mlua::Result<mlua::chunk::Chunk<'a>> {
        in_time(lua)?;
        let src = crate::skin::read_text(path).map_err(|e| mlua::Error::runtime(e.to_string()))?;
        Ok(text_chunk(lua, src, path))
    }

    /// The Lua state of a skin while it loads.
    pub struct Script {
        lua: Lua,
        frame: Rc<Frame>,
        fns: Option<Table>,
    }

    impl Script {
        /// A Lua state with the skin modules; `global` also exposes them as
        /// globals (JSON skins), `home` is where the skin's files are. What
        /// runs in it until its first frame has `budget` in all.
        pub fn new(home: &Home, global: bool, budget: Duration) -> mlua::Result<Script> {
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
                mlua::HookTriggers::new().every_nth_instruction(HOOK_EVERY),
                |lua, _| in_time(lua).map(|()| mlua::VmState::Continue),
            )?;
            arm(&lua, budget);
            install_patterns(&lua)?;
            let expired_fn = lua.create_function(|lua, ()| Ok(expired(lua)))?;
            lua.load(GUARD)
                .set_name("vibebms guard")
                .call::<()>((expired_fn, TOO_LONG))?;
            lua.load(COMPAT).set_name("vibebms compat").call::<()>(())?;
            let vb = lua.create_table()?;
            let frame = Rc::new(Frame::default());
            install(&lua, &vb, &frame)?;
            // `require`, `dofile`, `loadfile` and `io` read files below the
            // skins folder only (`Home`)
            let root = crate::platform::vfs::normalize(&home.root);
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
                Ok(LV::Function(source(lua, &path)?.into_function()?))
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
                    match source(lua, &path)?.into_function() {
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
                    source(lua, &path)?.call::<mlua::MultiValue>(())
                })?,
            )?;
            // io reads the skin's files; what scripts write goes nowhere
            let read = lua.create_function(move |lua, p: String| {
                in_time(lua)?;
                let data = find(&p, false).and_then(|p| crate::skin::read_text(&p).ok());
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
            r
        }

        /// Turn the evaluated skin table into JSON, keeping its functions.
        pub fn to_json(&mut self, v: LV) -> mlua::Result<Value> {
            let fns: Table = self
                .lua
                .load(WALK)
                .set_name("vibebms walk")
                .call((v.clone(), FN_MARK))?;
            self.fns = Some(fns);
            let mut left = JSON_BYTES;
            Ok(json(&v, &mut Vec::new(), &mut left)?.unwrap_or(Value::Null))
        }
    }

    /// What a skin table may take as JSON (a table it holds in several
    /// places is one table in Lua and one copy per place here, outside
    /// Lua's memory limit).
    const JSON_BYTES: usize = 64 << 20;
    /// Tables inside tables a skin table may have.
    const JSON_DEPTH: usize = 128;

    fn spend(left: &mut usize, n: usize) -> mlua::Result<()> {
        *left = left
            .checked_sub(n)
            .ok_or_else(|| mlua::Error::runtime("the skin table is too large"))?;
        Ok(())
    }

    fn json_text(s: &mlua::LuaString, left: &mut usize) -> mlua::Result<String> {
        spend(left, s.as_bytes().len())?;
        match s.to_str() {
            Ok(s) => Ok(s.to_string()),
            Err(_) => Err(mlua::Error::runtime(
                "the skin table has text that is not UTF-8",
            )),
        }
    }

    /// A Lua value as JSON: a table with a first entry is an array,
    /// another one an object (its keys strings); `None` for what JSON has
    /// not (a function, a table inside itself), which a table goes
    /// without. `path`: the tables `v` is in; `left`: what the whole may
    /// still take.
    fn json(
        v: &LV,
        path: &mut Vec<*const std::ffi::c_void>,
        left: &mut usize,
    ) -> mlua::Result<Option<Value>> {
        spend(left, std::mem::size_of::<Value>())?;
        Ok(Some(match v {
            LV::Nil => Value::Null,
            LV::Boolean(b) => Value::Bool(*b),
            LV::Integer(i) => Value::from(*i),
            LV::Number(n) => Value::from(*n),
            LV::String(s) => Value::String(json_text(s, left)?),
            LV::Table(t) => {
                let ptr = t.to_pointer();
                if path.contains(&ptr) {
                    return Ok(None);
                }
                if path.len() >= JSON_DEPTH {
                    return Err(mlua::Error::runtime("the skin table is nested too deep"));
                }
                path.push(ptr);
                let len = t.raw_len();
                let out = if len > 0 {
                    let mut a = Vec::new();
                    for i in 1..=len {
                        a.extend(json(&t.raw_get(i)?, path, left)?);
                    }
                    Value::Array(a)
                } else {
                    let mut m = serde_json::Map::new();
                    for pair in t.pairs::<LV, LV>() {
                        let (k, v) = pair?;
                        let Some(v) = json(&v, path, left)? else {
                            continue;
                        };
                        match k {
                            LV::String(k) => m.insert(json_text(&k, left)?, v),
                            LV::Function(_) | LV::Thread(_) | LV::UserData(_) => continue,
                            k => {
                                let msg =
                                    format!("the skin table has a {} for a name", k.type_name());
                                return Err(mlua::Error::runtime(msg));
                            }
                        };
                    }
                    Value::Object(m)
                };
                path.pop();
                out
            }
            _ => return Ok(None),
        }))
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
        /// The script of an object the player clicked is running.
        clicked: Cell<bool>,
        /// Game events that script asked for (`main_state.event_exec`).
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
                let mut timers = fr.set_timers.borrow_mut();
                let room = timers.len() < SET_TIMERS || timers.contains_key(&id);
                if room {
                    timers.insert(id, v);
                }
                Ok(room)
            },
        )?;
        // a game event (a play started, an option changed and saved, a
        // page opened ...) is the player's to ask for: a script's counts
        // while it runs for their click, a few a click
        put(lua, vb, frame, "event_exec", zero, |_, _, fr, id: i32| {
            let mut exec = fr.exec.borrow_mut();
            let taken = fr.clicked.get() && exec.len() < CLICK_EVENTS;
            if taken {
                exec.push(id);
            }
            Ok(taken)
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
        action: Function,
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
                match Script::new(&self.home, true, LOAD_TIME) {
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

        /// `customEvents`: the script `action` runs whenever `condition`
        /// holds, at most every `min_interval_ms` (a game event's id for
        /// an action is not run: those are the player's to ask for).
        pub fn custom_event(&mut self, action: &Value, condition: Option<&Value>, min_ms: i64) {
            let Some(action) = self.function(action, ScriptKind::Event) else {
                return;
            };
            let condition = condition.and_then(|c| self.function(c, ScriptKind::Bool));
            self.events.push(CustomEvent {
                action,
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
                late: Cell::new(false),
                late_frames: Cell::new(0),
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
        /// A script ran out of the frame's time.
        late: Cell<bool>,
        /// Frames in a row one did; at `LATE_FRAMES` the scripts are
        /// stopped (a skin would hold the game at a frame a second).
        late_frames: Cell<u32>,
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
            // (a text is drawn every frame: its start, of one of any length)
            LV::String(s) => {
                let b = s.as_bytes();
                Val::Str(String::from_utf8_lossy(&b[..b.len().min(TEXT_BYTES)]).into_owned())
            }
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
            let entry = self.entries.get(k).filter(|e| e.kind != ScriptKind::Event);
            let Some(e) = entry.filter(|_| !self.stopped()) else {
                return f(&Val::Nil);
            };
            let now = self.generation.get();
            if self.cache.borrow()[k].0 != now {
                let v = self.frame.with(st, || self.eval(k, e));
                self.cache.borrow_mut()[k] = (now, v);
            }
            f(&self.cache.borrow()[k].1)
        }

        fn stopped(&self) -> bool {
            self.late_frames.get() >= LATE_FRAMES
        }

        /// Call a script; an error that came of the frame's time running
        /// out is counted against the skin (`update`).
        fn call<R: mlua::FromLuaMulti>(&self, f: &Function) -> mlua::Result<R> {
            let r = f.call::<R>(());
            if r.is_err() && expired(&self.lua) {
                self.late.set(true);
            }
            r
        }

        fn eval(&self, k: usize, e: &Entry) -> Val {
            match self.call::<LV>(&e.f) {
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

        /// Game events the scripts of clicked objects asked for since the
        /// last call.
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
            if self.stopped() {
                return;
            }
            self.late_frames.set(if self.late.take() {
                self.late_frames.get() + 1
            } else {
                0
            });
            if self.stopped() {
                log::warn!("skin scripts stopped: out of time {LATE_FRAMES} frames in a row");
                return;
            }
            arm(&self.lua, FRAME_TIME);
            let now_us = now.saturating_mul(1000);
            self.frame.now_us.set(now_us);
            self.generation.set(self.generation.get() + 1);
            self.frame.with(st, || {
                let queued = std::mem::take(&mut *self.queued.borrow_mut());
                self.frame.clicked.set(true);
                for k in queued {
                    if let Err(e) = self.call::<()>(&self.entries[k].f) {
                        self.warn(k, &e);
                    }
                }
                self.frame.clicked.set(false);
                for ev in self.events.borrow_mut().iter_mut() {
                    let on = match &ev.condition {
                        Some(c) => self.call::<LV>(c).is_ok_and(|v| val(v).truthy()),
                        None => true,
                    };
                    let due = ev.last_us == i64::MIN
                        || (now_us - ev.last_us) / 1000 >= ev.min_interval_ms;
                    if on && due {
                        ev.last_us = now_us;
                        if let Err(e) = self.call::<()>(&ev.action) {
                            log::debug!("skin custom event: {e}");
                        }
                    }
                }
                for (id, f) in &self.timers {
                    let v = self.call::<LV>(f).map(|v| val(v).int()).ok().flatten();
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

        /// The home of a skin in `dir`, a folder of the skins folder.
        fn home(dir: &Path) -> Home {
            Home::of(&dir.join("s.luaskin"), dir.parent().unwrap())
        }

        /// A state whose scripts have a short time.
        fn script(dir: &Path) -> Script {
            Script::new(&home(dir), true, Duration::from_millis(300)).unwrap()
        }

        fn eval<T: mlua::FromLuaMulti>(code: &str) -> mlua::Result<T> {
            script(Path::new("/nonexistent/theme/skin"))
                .lua()
                .load(code)
                .eval::<T>()
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
            let s = script(&skin);
            let read = |name: &str| -> Option<String> {
                let code = format!(r#"local f = io.open("{name}") return f and f:read("a")"#);
                s.lua().load(code).eval().unwrap()
            };
            assert_eq!(read("own.txt").as_deref(), Some("own"));
            assert_eq!(read("link.txt"), None);
            std::fs::remove_dir_all(&dir).ok();
        }

        /// What a script names is looked for from the skin's folder up to
        /// the skins folder, and beatoraja's `skin/<theme>/...` in the
        /// folders on that way, for a skin in a theme's folder or in a
        /// folder of it; nothing above the skins folder is found.
        #[test]
        fn script_files_are_found_up_to_the_skins_folder() {
            let root =
                std::env::temp_dir().join(format!("vibebms-lua-find-{}", std::process::id()));
            let skins = root.join("skins");
            let files = [
                "skins/T/lib.lua",
                "skins/T/sub/own.lua",
                "skins/Shared/s.lua",
            ];
            for f in files.into_iter().chain(["config/c.lua"]) {
                std::fs::create_dir_all(root.join(f).parent().unwrap()).unwrap();
                std::fs::write(root.join(f), "return 1").unwrap();
            }
            let [lib, own, shared] = files.map(|f| root.join(f));
            let outside = root.join("config/c.lua");
            let find = |dir: &str, name: &str| resolve(&skins.join(dir), &skins, name, true);
            for dir in ["T", "T/sub"] {
                assert_eq!(find(dir, "lib.lua").as_ref(), Some(&lib), "{dir}");
                assert_eq!(find(dir, "skin/Any/lib.lua").as_ref(), Some(&lib), "{dir}");
                assert_eq!(
                    find(dir, "skin/Any/sub/own.lua").as_ref(),
                    Some(&own),
                    "{dir}"
                );
                assert_eq!(find(dir, "sub\\own.lua").as_ref(), Some(&own), "{dir}");
                assert_eq!(find(dir, "Shared/s.lua").as_ref(), Some(&shared), "{dir}");
                assert_eq!(find(dir, shared.to_str().unwrap()).as_ref(), Some(&shared));
                assert_eq!(find(dir, outside.to_str().unwrap()), None, "{dir}");
                assert_eq!(find(dir, "config/c.lua"), None, "{dir}");
                assert_eq!(find(dir, "\\\\host\\share\\c.lua"), None, "{dir}");
            }
            assert_eq!(find("T", "../Shared/s.lua").as_ref(), Some(&shared));
            assert_eq!(find("T", "../../config/c.lua"), None);
            assert_eq!(find("T/sub", "own.lua").as_ref(), Some(&own));
            assert_eq!(find("T/sub", "../../../config/c.lua"), None);
            // a skin file in the skins folder itself
            assert_eq!(find("", "T/lib.lua").as_ref(), Some(&lib));
            assert_eq!(find("", "../config/c.lua"), None);
            std::fs::remove_dir_all(&root).ok();
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
            let s = script(Path::new("/nonexistent/theme/skin"));
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
            let s = script(dir);
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

        /// The scripts `build` registers, as a loaded skin's.
        fn runtime(setup: &str, build: impl FnOnce(&mut Builder)) -> Rc<Runtime> {
            let dir = Path::new("/nonexistent/theme/skin");
            let s = script(dir);
            s.lua().load(setup).exec().unwrap();
            let mut b = Builder::new(Some(s), &home(dir));
            build(&mut b);
            b.finish().unwrap()
        }

        fn code(code: &str) -> Value {
            Value::String(code.into())
        }

        /// A script's game events count while it runs for a click, a few
        /// a click; a value, a timer or a custom event asks for none.
        #[test]
        fn game_events_come_of_clicks_only() {
            let (mut click, mut value) = (0, 0);
            let rt = runtime("", |b| {
                let many = code("for i = 1, 100 do event_exec(15) end");
                click = b.register(&many, ScriptKind::Event).unwrap();
                value = b
                    .register(&code("event_exec(16)"), ScriptKind::Bool)
                    .unwrap();
                b.custom_event(&code("event_exec(17)"), None, 0);
                b.custom_event(&Value::from(18), None, 0);
                b.custom_timer(900, &code("event_exec(19) and 0 or 0"));
            });
            let offsets = HashMap::new();
            let ov = Overlay {
                st: &St,
                rt: Some(&rt),
                offsets: &offsets,
            };
            for frame in 0..3 {
                rt.update(&St, frame * 16);
                assert!(!ov.option(value));
            }
            assert_eq!(rt.take_events(), Vec::<i32>::new());
            assert!(rt.click(click));
            rt.update(&St, 48);
            assert!(!ov.option(value));
            assert_eq!(rt.take_events(), vec![15; CLICK_EVENTS]);
        }

        /// Scripts that run out of a frame's time frame after frame run
        /// no more (the skin would hold the game at a frame a second).
        #[test]
        fn scripts_out_of_time_frame_after_frame_are_stopped() {
            let mut spin = 0;
            let rt = runtime("runs = 0", |b| {
                let endless = code("(function() runs = runs + 1 while true do end end)()");
                spin = b.register(&endless, ScriptKind::Int).unwrap();
            });
            let offsets = HashMap::new();
            let ov = Overlay {
                st: &St,
                rt: Some(&rt),
                offsets: &offsets,
            };
            for frame in 0..LATE_FRAMES as i64 + 2 {
                rt.update(&St, frame * 16);
                // (the frame's time is over while the value is worked out)
                arm(&rt.lua, Duration::ZERO);
                assert_eq!(ov.number(spin), None);
            }
            assert_eq!(rt.lua.globals().get::<u32>("runs").unwrap(), LATE_FRAMES);
        }

        /// What scripts leave on the Rust side (outside Lua's memory
        /// limit) is bounded: the timers they set, the text they give.
        #[test]
        fn what_scripts_leave_outside_lua_is_bounded() {
            let mut text = 0;
            let rt = runtime("", |b| {
                let long = code(
                    "(function()
                       for i = 1, 100000 do set_timer(i, 0) end
                       return ('x'):rep(1000000)
                     end)()",
                );
                text = b.register(&long, ScriptKind::Str).unwrap();
            });
            let offsets = HashMap::new();
            let ov = Overlay {
                st: &St,
                rt: Some(&rt),
                offsets: &offsets,
            };
            rt.update(&St, 0);
            assert_eq!(ov.text(text).map(|t| t.len()), Some(TEXT_BYTES));
            assert_eq!(rt.frame.set_timers.borrow().len(), SET_TIMERS);
        }

        #[test]
        fn a_runaway_pattern_is_stopped() {
            let r: mlua::Result<LV> = eval(r#"return ("a"):rep(5000):find(".-.-.-.-.-x")"#);
            assert!(r.unwrap_err().to_string().contains("too long"));
        }

        /// What ran on with the hook off, or in one library call the hook
        /// counts as an instruction, ends with the script's time (or at
        /// once).
        #[test]
        fn the_time_limit_holds_where_the_hook_does_not_reach() {
            let t = std::time::Instant::now();
            let endless = "function() while true do end end";
            let made_up = "setmetatable({}, { __len = function() return math.maxinteger - 1 end,
                __index = rawlen, __newindex = rawequal })";
            // (its length is made up without a metatable)
            let keys: String = (0..63).map(|i| format!("[1 << {i}] = 1, ")).collect();
            let sparse = format!("{{ {keys}[math.maxinteger] = 1 }}");
            let len = eval::<i64>(&format!("return #{sparse}"));
            assert_eq!(len.unwrap(), i64::MAX);
            for (code, error) in [
                // a message handler, a reader function, closing coroutines
                (format!("xpcall({endless}, {endless})"), TOO_LONG),
                (format!("while true do load({endless}) end"), TOO_LONG),
                (
                    format!(
                        "local function co()
                           local c = coroutine.create(function()
                             local x <close> = setmetatable({{}}, {{ __close = {endless} }})
                             coroutine.yield()
                           end)
                           coroutine.resume(c)
                           return c
                         end
                         local cs = {{}}
                         for i = 1, 200 do cs[i] = co() end
                         while true do for i = 1, 200 do coroutine.close(cs[i]) end end"
                    ),
                    TOO_LONG,
                ),
                // loops of the table library over what is not there
                (
                    "table.move({}, 1, math.maxinteger - 1, 2)".to_string(),
                    TOO_LONG,
                ),
                (format!("table.remove({made_up}, 1)"), TOO_LONG),
                (format!("table.insert({made_up}, 1, 0)"), TOO_LONG),
                (format!("table.concat({made_up})"), TOO_LONG),
                (format!("table.sort({made_up})"), "array too big"),
                (format!("table.remove({sparse}, 1)"), TOO_LONG),
                (
                    "getmetatable('').__newindex = rawequal
                     table.move('a', 1, math.maxinteger - 1, 2, 'b')"
                        .to_string(),
                    TOO_LONG,
                ),
            ] {
                let r: mlua::Result<()> = eval(&code);
                let e = r.unwrap_err().to_string();
                assert!(e.contains(error), "{code}: {e}");
            }
            assert_eq!(
                eval::<String>("return ('').rep('', 1 << 62, '')").unwrap(),
                ""
            );
            assert_eq!(
                eval::<String>("return ('ab'):rep(2, '-')").unwrap(),
                "ab-ab"
            );
            assert!(t.elapsed() < std::time::Duration::from_secs(60));
            // library calls that take long, a few instructions apart
            let t = std::time::Instant::now();
            let r: mlua::Result<()> =
                eval("local s = ('x'):rep(8000000) while true do s:upper() end");
            assert!(r.unwrap_err().to_string().contains("too long"));
            assert!(t.elapsed() < std::time::Duration::from_secs(5));
        }

        /// A finalizer would run with the hook off: tables get none, as
        /// in beatoraja's Lua, and their metatables are as they were set.
        #[test]
        fn tables_have_no_finalizers() {
            let s = script(Path::new("/nonexistent/theme/skin"));
            let kept: bool = s
                .lua()
                .load(
                    "ran = false
                     local mt = { __gc = function() ran = true end, __index = { a = 1 } }
                     local t = setmetatable({}, mt)
                     local kept = t.a == 1 and getmetatable(t) == mt and mt.__gc ~= nil
                     t = setmetatable({}, { __gc = false })
                     getmetatable(t).__gc = function() ran = true end
                     t = nil
                     collectgarbage()
                     return kept and collectgarbage('count') > 0
                       and not pcall(setmetatable, setmetatable({}, { __metatable = 1 }), mt)",
                )
                .eval()
                .unwrap();
            assert!(kept);
            s.lua().gc_collect().unwrap();
            s.lua().gc_collect().unwrap();
            assert!(!s.lua().globals().get::<bool>("ran").unwrap());
        }

        /// The table functions done in Lua (a table with a metatable, a
        /// long one) give what Lua's own give.
        #[test]
        fn table_functions_as_lua_whatever_the_table() {
            let v: String = eval(
                r#"local out = {}
                for _, make in ipairs({
                  function(t) return t end,
                  function(t) return setmetatable(t, {}) end,
                }) do
                  local t = make({ 3, 1, 2 })
                  table.insert(t, 5)
                  table.insert(t, 1, 4)
                  local last, second = table.remove(t), table.remove(t, 2)
                  table.sort(t, function(a, b) return a > b end)
                  local s = table.concat(t, ",") .. "|" .. table.concat(t, "-", 2, 3)
                  local m = table.move(t, 1, 3, 2)
                  local n = table.move(t, 2, 4, 1, make({}))
                  local e = { pcall(table.insert, t, 9, 0) }
                  local f = { pcall(table.concat, make({ 1, {}, 3 })) }
                  out[#out + 1] = table.concat({ last, second, s, table.concat(m, ","),
                    table.concat(n, ","), tostring(e[1]), tostring(f[1]),
                    tostring(table.remove(make({}))), table.unpack(t) }, " ")
                end
                assert(out[1] == out[2], out[1] .. " / " .. out[2])
                local long = {}
                for i = 1, (1 << 20) + 2 do long[i] = i end
                table.insert(long, 1, 0)
                assert(table.remove(long, 1) == 0 and table.remove(long) == (1 << 20) + 2)
                assert(#table.move(long, 1, #long, 1, {}) == (1 << 20) + 1)
                assert(#table.concat(long, "", 1 << 20) == 14 and not pcall(table.sort, long))
                return out[1]"#,
            )
            .unwrap();
            assert_eq!(v, "5 3 4,2,1|2-1 4,4,2,1 4,2,1 false false nil 4 4 2 1");
        }

        /// A skin table is one copy per place a table is held in as JSON:
        /// it is refused past a size, a table inside itself left out.
        #[test]
        fn the_skin_table_as_json_is_bounded() {
            let json = |code: &str| {
                let mut s = script(Path::new("/nonexistent/theme/skin"));
                let v: LV = s.lua().load(code).eval().unwrap();
                s.to_json(v).map_err(|e| e.to_string())
            };
            let shared = json("local t = { 1 } for i = 1, 60 do t = { t, t } end return t");
            assert!(shared.unwrap_err().contains("too large"));
            let deep = json("local t = {} for i = 1, 200 do t = { t } end return t");
            assert!(deep.unwrap_err().contains("too deep"));
            let v = json(
                "local t = { name = 'n', list = { 1, 2.5, 'x', false }, f = print, [print] = 1,
                   co = coroutine.create(print) }
                 t.self = t
                 t.list[5] = t.list
                 return t",
            )
            .unwrap();
            assert_eq!(v["list"], serde_json::json!([1, 2.5, "x", false]));
            assert!(v["f"].as_str().is_some_and(|f| f.starts_with(FN_MARK)));
            assert_eq!(
                (&v["name"], v.as_object().unwrap().len()),
                (&Value::from("n"), 3)
            );
            assert!(json("return { [2] = 'a' }").is_err());
            assert!(json("return { '\\xff' }").is_err());
            assert_eq!(json("return 1 / 0").unwrap(), Value::Null);
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
