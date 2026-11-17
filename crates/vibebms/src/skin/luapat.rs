//! Lua 5.4's pattern matching (`string.find`, `match`, `gmatch`, `gsub`)
//! with a step budget.
//!
//! Lua's own matcher runs in C, where the instruction hook that stops a
//! skin script that runs too long cannot reach: a pattern such as
//! `(".-"):rep(8) .. "x"` on a long string backtracks for hours. This one
//! follows `lstrlib.c` (same results, same errors) and gives up after
//! `BUDGET` steps per call.
//!
//! A port of Lua 5.4's `lstrlib.c`, Copyright (C) 1994-2024 Lua.org,
//! PUC-Rio, under the MIT licence (full text in THIRD_PARTY_NOTICES.md).

/// Steps one call may take (well under a second).
pub const BUDGET: u64 = 20_000_000;
const MAXCAPTURES: usize = 32;
const MAXCCALLS: usize = 200;
const SPECIALS: &[u8] = b"^$*+?.([%-";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CapLen {
    Unfinished,
    Position,
    Len(usize),
}

/// A capture: a string's bytes or a position (1-based).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cap<'a> {
    Str(&'a [u8]),
    Pos(i64),
}

pub type Result<T> = std::result::Result<T, String>;

struct State<'a> {
    src: &'a [u8],
    pat: &'a [u8],
    level: usize,
    capture: [(usize, CapLen); MAXCAPTURES],
    depth: usize,
    steps: u64,
}

fn class_matches(c: u8, cl: u8) -> bool {
    let res = match cl.to_ascii_lowercase() {
        b'a' => c.is_ascii_alphabetic(),
        b'c' => c < 0x20 || c == 0x7f,
        b'd' => c.is_ascii_digit(),
        b'g' => c.is_ascii_graphic(),
        b'l' => c.is_ascii_lowercase(),
        b'p' => c.is_ascii_punctuation(),
        b's' => matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r'),
        b'u' => c.is_ascii_uppercase(),
        b'w' => c.is_ascii_alphanumeric(),
        b'x' => c.is_ascii_hexdigit(),
        _ => return cl == c,
    };
    if cl.is_ascii_uppercase() {
        !res
    } else {
        res
    }
}

impl<'a> State<'a> {
    fn new(src: &'a [u8], pat: &'a [u8]) -> State<'a> {
        State {
            src,
            pat,
            level: 0,
            capture: [(0, CapLen::Unfinished); MAXCAPTURES],
            depth: MAXCCALLS,
            steps: 0,
        }
    }

    /// The pattern byte at `i` (0 past the end, as C's terminator).
    fn p(&self, i: usize) -> u8 {
        self.pat.get(i).copied().unwrap_or(0)
    }

    fn step(&mut self) -> Result<()> {
        self.steps += 1;
        if self.steps > BUDGET {
            return Err("pattern matching takes too long".into());
        }
        Ok(())
    }

    fn class_end(&self, mut p: usize) -> Result<usize> {
        let c = self.p(p);
        p += 1;
        if c == b'%' {
            if p >= self.pat.len() {
                return Err("malformed pattern (ends with '%')".into());
            }
            return Ok(p + 1);
        }
        if c == b'[' {
            if self.p(p) == b'^' {
                p += 1;
            }
            loop {
                if p >= self.pat.len() {
                    return Err("malformed pattern (missing ']')".into());
                }
                let c = self.p(p);
                p += 1;
                if c == b'%' && p < self.pat.len() {
                    p += 1;
                }
                if self.p(p) == b']' {
                    break;
                }
            }
            return Ok(p + 1);
        }
        Ok(p)
    }

    /// `p` at the class's `[`, `ec` at its `]`.
    fn bracket_class(&self, c: u8, mut p: usize, ec: usize) -> bool {
        let mut sig = true;
        if self.p(p + 1) == b'^' {
            sig = false;
            p += 1;
        }
        loop {
            p += 1;
            if p >= ec {
                break;
            }
            if self.p(p) == b'%' {
                p += 1;
                if class_matches(c, self.p(p)) {
                    return sig;
                }
            } else if self.p(p + 1) == b'-' && p + 2 < ec {
                p += 2;
                if self.p(p - 2) <= c && c <= self.p(p) {
                    return sig;
                }
            } else if self.p(p) == c {
                return sig;
            }
        }
        !sig
    }

    fn single(&self, s: usize, p: usize, ep: usize) -> bool {
        let Some(&c) = self.src.get(s) else {
            return false;
        };
        match self.p(p) {
            b'.' => true,
            b'%' => class_matches(c, self.p(p + 1)),
            b'[' => self.bracket_class(c, p, ep - 1),
            pc => pc == c,
        }
    }

    fn balance(&mut self, s: usize, p: usize) -> Result<Option<usize>> {
        if p + 1 >= self.pat.len() {
            return Err("malformed pattern (missing arguments to '%b')".into());
        }
        if self.src.get(s) != Some(&self.p(p)) {
            return Ok(None);
        }
        let (b, e) = (self.p(p), self.p(p + 1));
        let mut cont = 1;
        let mut s = s + 1;
        while s < self.src.len() {
            self.step()?;
            let c = self.src[s];
            if c == e {
                cont -= 1;
                if cont == 0 {
                    return Ok(Some(s + 1));
                }
            } else if c == b {
                cont += 1;
            }
            s += 1;
        }
        Ok(None)
    }

    fn max_expand(&mut self, s: usize, p: usize, ep: usize) -> Result<Option<usize>> {
        let mut i = 0usize;
        while self.single(s + i, p, ep) {
            self.step()?;
            i += 1;
        }
        loop {
            if let Some(r) = self.do_match(s + i, ep + 1)? {
                return Ok(Some(r));
            }
            if i == 0 {
                return Ok(None);
            }
            i -= 1;
        }
    }

    fn min_expand(&mut self, mut s: usize, p: usize, ep: usize) -> Result<Option<usize>> {
        loop {
            if let Some(r) = self.do_match(s, ep + 1)? {
                return Ok(Some(r));
            }
            if self.single(s, p, ep) {
                s += 1;
            } else {
                return Ok(None);
            }
        }
    }

    fn start_capture(&mut self, s: usize, p: usize, what: CapLen) -> Result<Option<usize>> {
        if self.level >= MAXCAPTURES {
            return Err("too many captures".into());
        }
        self.capture[self.level] = (s, what);
        self.level += 1;
        let r = self.do_match(s, p)?;
        if r.is_none() {
            self.level -= 1;
        }
        Ok(r)
    }

    fn end_capture(&mut self, s: usize, p: usize) -> Result<Option<usize>> {
        let l = (0..self.level)
            .rev()
            .find(|l| self.capture[*l].1 == CapLen::Unfinished)
            .ok_or("invalid pattern capture")?;
        self.capture[l].1 = CapLen::Len(s - self.capture[l].0);
        let r = self.do_match(s, p)?;
        if r.is_none() {
            self.capture[l].1 = CapLen::Unfinished;
        }
        Ok(r)
    }

    fn match_capture(&self, s: usize, l: u8) -> Result<Option<usize>> {
        let l = (l as usize).wrapping_sub(b'1' as usize);
        let len = match self.capture.get(l).filter(|_| l < self.level) {
            Some((_, CapLen::Len(n))) => *n,
            // (Lua compares a position's length as a huge size: no match)
            Some((_, CapLen::Position)) => return Ok(None),
            _ => return Err(format!("invalid capture index %{}", l.wrapping_add(1))),
        };
        let start = self.capture[l].0;
        if self.src.len() - s >= len && self.src[start..start + len] == self.src[s..s + len] {
            Ok(Some(s + len))
        } else {
            Ok(None)
        }
    }

    fn do_match(&mut self, s: usize, p: usize) -> Result<Option<usize>> {
        if self.depth == 0 {
            return Err("pattern too complex".into());
        }
        self.depth -= 1;
        let r = self.match_at(s, p);
        self.depth += 1;
        r
    }

    fn match_at(&mut self, mut s: usize, mut p: usize) -> Result<Option<usize>> {
        loop {
            self.step()?;
            if p >= self.pat.len() {
                return Ok(Some(s));
            }
            match self.p(p) {
                b'(' => {
                    return if self.p(p + 1) == b')' {
                        self.start_capture(s, p + 2, CapLen::Position)
                    } else {
                        self.start_capture(s, p + 1, CapLen::Unfinished)
                    };
                }
                b')' => return self.end_capture(s, p + 1),
                b'$' if p + 1 == self.pat.len() => {
                    return Ok((s == self.src.len()).then_some(s));
                }
                b'%' if self.p(p + 1) == b'b' => match self.balance(s, p + 2)? {
                    Some(e) => {
                        s = e;
                        p += 4;
                        continue;
                    }
                    None => return Ok(None),
                },
                b'%' if self.p(p + 1) == b'f' => {
                    p += 2;
                    if self.p(p) != b'[' {
                        return Err("missing '[' after '%f' in pattern".into());
                    }
                    let ep = self.class_end(p)?;
                    let prev = if s == 0 { 0 } else { self.src[s - 1] };
                    let cur = self.src.get(s).copied().unwrap_or(0);
                    if !self.bracket_class(prev, p, ep - 1) && self.bracket_class(cur, p, ep - 1) {
                        p = ep;
                        continue;
                    }
                    return Ok(None);
                }
                b'%' if self.p(p + 1).is_ascii_digit() => {
                    match self.match_capture(s, self.p(p + 1))? {
                        Some(e) => {
                            s = e;
                            p += 2;
                            continue;
                        }
                        None => return Ok(None),
                    }
                }
                _ => {}
            }
            let ep = self.class_end(p)?;
            let epc = self.p(ep);
            if !self.single(s, p, ep) {
                if matches!(epc, b'*' | b'?' | b'-') {
                    p = ep + 1;
                    continue;
                }
                return Ok(None);
            }
            match epc {
                b'?' => {
                    if let Some(r) = self.do_match(s + 1, ep + 1)? {
                        return Ok(Some(r));
                    }
                    p = ep + 1;
                }
                b'+' => return self.max_expand(s + 1, p, ep),
                b'*' => return self.max_expand(s, p, ep),
                b'-' => return self.min_expand(s, p, ep),
                _ => {
                    s += 1;
                    p = ep;
                }
            }
        }
    }

    /// Capture `i` of a match `s..e` (the whole match when there are none).
    fn capture_at(&self, i: usize, s: usize, e: usize) -> Result<Cap<'a>> {
        if i >= self.level {
            if i == 0 {
                return Ok(Cap::Str(&self.src[s..e]));
            }
            return Err(format!("invalid capture index %{}", i + 1));
        }
        let (start, len) = self.capture[i];
        match len {
            CapLen::Unfinished => Err("unfinished capture".into()),
            CapLen::Position => Ok(Cap::Pos(start as i64 + 1)),
            CapLen::Len(n) => Ok(Cap::Str(&self.src[start..start + n])),
        }
    }

    fn captures(&self, s: usize, e: usize, whole: bool) -> Result<Vec<Cap<'a>>> {
        let n = if self.level == 0 && whole {
            1
        } else {
            self.level
        };
        (0..n).map(|i| self.capture_at(i, s, e)).collect()
    }

    fn reset(&mut self) {
        self.level = 0;
        self.depth = MAXCCALLS;
    }
}

/// Lua's `posrelatI` minus one: a start index (1-based, negative from the
/// end) as a byte offset.
fn start_index(init: i64, len: usize) -> usize {
    let len = len as i64;
    let pos = if init > 0 {
        init
    } else if init == 0 || init < -len {
        1
    } else {
        len + init + 1
    };
    (pos - 1) as usize
}

/// `string.find` (`find`) or `string.match`: `None` when nothing matches,
/// else for find the 1-based start and end then the captures.
pub fn find<'a>(
    s: &'a [u8],
    pat: &'a [u8],
    init: i64,
    plain: bool,
    find: bool,
) -> Result<Option<(usize, usize, Vec<Cap<'a>>)>> {
    let init = start_index(init, s.len());
    if init > s.len() {
        return Ok(None);
    }
    if find && (plain || !pat.iter().any(|c| SPECIALS.contains(c))) {
        let hit = if pat.is_empty() {
            Some(init)
        } else {
            s[init..]
                .windows(pat.len())
                .position(|w| w == pat)
                .map(|i| i + init)
        };
        return Ok(hit.map(|i| (i + 1, i + pat.len(), Vec::new())));
    }
    let anchor = pat.first() == Some(&b'^');
    let p0 = anchor as usize;
    let mut st = State::new(s, pat);
    let mut s1 = init;
    loop {
        st.reset();
        if let Some(e) = st.do_match(s1, p0)? {
            let caps = st.captures(s1, e, !find)?;
            return Ok(Some((s1 + 1, e, caps)));
        }
        s1 += 1;
        if s1 > s.len() || anchor {
            return Ok(None);
        }
    }
}

/// One step of `string.gmatch`: from `*pos`, the next match not ending
/// where the last one did (`last`). Its captures, or `None` at the end.
pub fn gmatch_next<'a>(
    s: &'a [u8],
    pat: &'a [u8],
    pos: &mut usize,
    last: &mut Option<usize>,
) -> Result<Option<Vec<Cap<'a>>>> {
    let mut st = State::new(s, pat);
    let mut src = *pos;
    while src <= s.len() {
        st.reset();
        if let Some(e) = st.do_match(src, 0)? {
            if Some(e) != *last {
                *pos = e;
                *last = Some(e);
                return Ok(Some(st.captures(src, e, true)?));
            }
        }
        src += 1;
    }
    *pos = s.len() + 1;
    Ok(None)
}

/// Where `string.gmatch` starts (`init`, 1-based).
pub fn gmatch_start(init: i64, len: usize) -> usize {
    start_index(init, len).min(len + 1)
}

/// What a function or table replacement gives for a match's captures.
pub type ReplFn<'r> = dyn FnMut(&[Cap]) -> Result<Option<Vec<u8>>> + 'r;

/// A replacement of `string.gsub`.
pub enum Repl<'r> {
    /// A string with `%0`-`%9` and `%%`.
    Str(&'r [u8]),
    /// A function (given the captures) or a table (indexed by the first):
    /// `Some` bytes replace the match, `None` keeps it.
    Call(&'r mut ReplFn<'r>),
}

fn cap_bytes(c: &Cap) -> Vec<u8> {
    match c {
        Cap::Str(b) => b.to_vec(),
        Cap::Pos(n) => n.to_string().into_bytes(),
    }
}

/// `string.gsub`: the new string and the number of matches replaced.
pub fn gsub(s: &[u8], pat: &[u8], mut repl: Repl, max_n: Option<i64>) -> Result<(Vec<u8>, i64)> {
    let anchor = pat.first() == Some(&b'^');
    let p0 = anchor as usize;
    let max_n = max_n.unwrap_or(s.len() as i64 + 1);
    let mut st = State::new(s, pat);
    let mut out = Vec::with_capacity(s.len());
    let (mut src, mut n, mut last) = (0usize, 0i64, None);
    while n < max_n {
        st.reset();
        match st.do_match(src, p0)? {
            Some(e) if Some(e) != last => {
                n += 1;
                match &mut repl {
                    Repl::Str(r) => {
                        let mut i = 0;
                        while i < r.len() {
                            if r[i] != b'%' {
                                out.push(r[i]);
                                i += 1;
                                continue;
                            }
                            i += 1;
                            match r.get(i) {
                                Some(b'%') => out.push(b'%'),
                                Some(d) if d.is_ascii_digit() => {
                                    let c = if *d == b'0' {
                                        Cap::Str(&s[src..e])
                                    } else {
                                        st.capture_at((d - b'1') as usize, src, e)?
                                    };
                                    out.extend(cap_bytes(&c));
                                }
                                _ => return Err("invalid use of '%' in replacement string".into()),
                            }
                            i += 1;
                        }
                    }
                    Repl::Call(f) => {
                        let c = st.captures(src, e, true)?;
                        match f(&c)? {
                            Some(v) => out.extend(v),
                            None => out.extend_from_slice(&s[src..e]),
                        }
                    }
                }
                src = e;
                last = Some(e);
            }
            _ if src < s.len() => {
                out.push(s[src]);
                src += 1;
            }
            _ => break,
        }
        if anchor {
            break;
        }
    }
    out.extend_from_slice(&s[src.min(s.len())..]);
    Ok((out, n))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m<'a>(s: &'a str, p: &'a str) -> Option<Vec<Cap<'a>>> {
        find(s.as_bytes(), p.as_bytes(), 1, false, false)
            .unwrap()
            .map(|r| r.2)
    }
    fn st(c: &Cap) -> String {
        String::from_utf8_lossy(&cap_bytes(c)).into_owned()
    }
    fn ms(s: &str, p: &str) -> Vec<String> {
        m(s, p).unwrap_or_default().iter().map(st).collect()
    }

    #[test]
    fn as_lua_matches() {
        assert_eq!(ms("  hello world ", "^%s*(.-)%s*$"), ["hello world"]);
        assert_eq!(ms("key = value", "(%w+)%s*=%s*(%w+)"), ["key", "value"]);
        assert_eq!(ms("THE (quick) fox", "%((%a+)%)"), ["quick"]);
        assert_eq!(ms("f(a(b)c)d", "%b()"), ["(a(b)c)"]);
        assert_eq!(ms("hello", "()ll()"), ["3", "5"]);
        assert_eq!(ms("abcabc", "(abc)%1"), ["abc"]);
        assert!(m("abab", "()b%1").is_none());
        assert_eq!(ms("THE (quick) fox", "%f[%a]%a+"), ["THE"]);
        assert_eq!(ms("x = 0x1F;", "[%x]+;"), ["1F;"]);
        assert_eq!(ms("a-b", "[a%-]+"), ["a-"]);
        assert_eq!(ms("2024-11-17", "(%d+)-(%d+)-(%d+)"), ["2024", "11", "17"]);
        assert_eq!(ms("abc", "[^a]+"), ["bc"]);
        assert_eq!(ms("aaa", "a-b"), Vec::<String>::new());
        assert!(m("abc", "^b").is_none());
        let f = find(b"hello world", b"o w", 1, false, true)
            .unwrap()
            .unwrap();
        assert_eq!((f.0, f.1), (5, 7));
        let f = find(b"a.b", b".", 1, true, true).unwrap().unwrap();
        assert_eq!((f.0, f.1), (2, 2));
        let f = find(b"abc", b"", 10, false, true).unwrap();
        assert!(f.is_none());
        let f = find(b"abcabc", b"b", -2, false, true).unwrap().unwrap();
        assert_eq!(f.0, 5);
        for bad in ["%", "[a", "(()", "%1", "a)", "%bx", "%fa"] {
            assert!(
                find(b"abc", bad.as_bytes(), 1, false, false).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn as_lua_gsub_and_gmatch() {
        let g = |s: &str, p: &str, r: &str| {
            let (v, n) = gsub(s.as_bytes(), p.as_bytes(), Repl::Str(r.as_bytes()), None).unwrap();
            (String::from_utf8(v).unwrap(), n)
        };
        assert_eq!(g("hello world", "o", "0"), ("hell0 w0rld".into(), 2));
        assert_eq!(
            g("hello world", "(%w+)", "<%1>"),
            ("<hello> <world>".into(), 2)
        );
        assert_eq!(g("abc", "", "-"), ("-a-b-c-".into(), 4));
        assert_eq!(g("abc", "^", ">"), (">abc".into(), 1));
        assert_eq!(g("a,b", "%w", "%0%0"), ("aa,bb".into(), 2));
        assert_eq!(g("50%", "%%", "%%%%"), ("50%%".into(), 1));
        assert!(gsub(b"a", b"a", Repl::Str(b"%x"), None).is_err());
        let (v, n) = gsub(b"aaa", b"a", Repl::Str(b"b"), Some(2)).unwrap();
        assert_eq!((v, n), (b"bba".to_vec(), 2));
        let mut up =
            |c: &[Cap]| Ok((c[0] != Cap::Str(b"b")).then(|| cap_bytes(&c[0]).to_ascii_uppercase()));
        let (v, _) = gsub(b"abc", b"%a", Repl::Call(&mut up), None).unwrap();
        assert_eq!(v, b"AbC");
        let s = b"one two  three";
        let (mut pos, mut last, mut words) = (gmatch_start(1, s.len()), None, Vec::new());
        while let Some(c) = gmatch_next(s, b"%a*", &mut pos, &mut last).unwrap() {
            words.push(st(&c[0]));
        }
        // as Lua 5.4: an empty match right after the previous one is skipped
        assert_eq!(words, ["one", "two", "", "three"]);
    }

    #[test]
    fn a_runaway_pattern_stops() {
        let s = vec![b'a'; 5000];
        let p = b".-.-.-.-.-x";
        let t = std::time::Instant::now();
        let r = find(&s, p, 1, false, false);
        assert!(r.is_err());
        assert!(t.elapsed() < std::time::Duration::from_secs(5));
        // deep nesting is refused as in Lua
        let deep = "(".repeat(300) + &")".repeat(300);
        assert!(find(b"x", deep.as_bytes(), 1, false, false).is_err());
    }
}
