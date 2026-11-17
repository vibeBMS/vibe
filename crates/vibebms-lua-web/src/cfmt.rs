//! C's `strtod` and the float conversions of `printf` (`%f %e %g %a`), on
//! Rust's exactly rounded formatting and parsing.

pub const F_MINUS: i32 = 1;
pub const F_PLUS: i32 = 2;
pub const F_SPACE: i32 = 4;
pub const F_HASH: i32 = 8;
pub const F_ZERO: i32 = 16;

/// The number `b` starts with (after white space) and the bytes it takes,
/// as C's `strtod` reads it: decimal or hexadecimal (`0x1.8p3`). Lua turns
/// down "inf" and "nan" itself.
pub fn strtod(b: &[u8]) -> (f64, usize) {
    parse(b).unwrap_or((0.0, 0))
}

fn parse(b: &[u8]) -> Option<(f64, usize)> {
    let mut i = 0;
    while i < b.len() && b[i].is_ascii_whitespace() {
        i += 1;
    }
    let start = i;
    let neg = b.get(i) == Some(&b'-');
    if matches!(b.get(i), Some(b'-' | b'+')) {
        i += 1;
    }
    if b.get(i) == Some(&b'0') && matches!(b.get(i + 1), Some(b'x' | b'X')) {
        if let Some((v, end)) = hex(b, i + 2) {
            return Some((if neg { -v } else { v }, end));
        }
        // "0x" and no digit: the "0"
        return Some((if neg { -0.0 } else { 0.0 }, i + 1));
    }
    let mut j = i;
    let mut digits = 0;
    while j < b.len() && b[j].is_ascii_digit() {
        j += 1;
        digits += 1;
    }
    if b.get(j) == Some(&b'.') {
        j += 1;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return None;
    }
    if matches!(b.get(j), Some(b'e' | b'E')) {
        let mut k = j + 1;
        if matches!(b.get(k), Some(b'-' | b'+')) {
            k += 1;
        }
        if b.get(k).is_some_and(|c| c.is_ascii_digit()) {
            while k < b.len() && b[k].is_ascii_digit() {
                k += 1;
            }
            j = k;
        }
    }
    let txt = std::str::from_utf8(&b[start..j]).ok()?;
    // Rust takes "1." and ".5" but not "+.5" with a dot alone: normalise
    let txt = txt.replace("+.", "+0.").replace("-.", "-0.");
    let v = match txt.parse::<f64>() {
        Ok(v) => v,
        Err(_) => {
            let t = txt.strip_suffix('.').unwrap_or(&txt);
            t.parse::<f64>().ok()?
        }
    };
    Some((v, j))
}

/// The hexadecimal digits from `i` on (with a dot and a `p` exponent).
fn hex(b: &[u8], mut i: usize) -> Option<(f64, usize)> {
    // the first 15 significant digits exactly, the rest only counted (as
    // a sticky bit, enough for correct rounding in practice)
    let (mut m, mut e, mut any, mut dot, mut sig, mut sticky) =
        (0u64, 0i64, false, false, 0, false);
    while i < b.len() {
        let c = b[i];
        if c == b'.' && !dot {
            dot = true;
        } else if let Some(d) = (c as char).to_digit(16) {
            any = true;
            if sig < 15 {
                if m != 0 || d != 0 {
                    sig += 1;
                }
                m = m * 16 + d as u64;
                if dot {
                    e -= 4;
                }
            } else {
                sticky |= d != 0;
                if !dot {
                    e += 4;
                }
            }
        } else {
            break;
        }
        i += 1;
    }
    if !any {
        return None;
    }
    if matches!(b.get(i), Some(b'p' | b'P')) {
        let mut k = i + 1;
        let eneg = b.get(k) == Some(&b'-');
        if matches!(b.get(k), Some(b'-' | b'+')) {
            k += 1;
        }
        let ds = k;
        let mut x = 0i64;
        while k < b.len() && b[k].is_ascii_digit() {
            x = (x * 10 + (b[k] - b'0') as i64).min(1 << 20);
            k += 1;
        }
        if k > ds {
            e += if eneg { -x } else { x };
            i = k;
        }
    }
    let m = (m << 1) | sticky as u64;
    let e = (e - 1).clamp(-1 << 20, 1 << 20) as i32;
    Some((ldexp(m as f64, e), i))
}

/// `x` × 2^`e`, in steps that neither overflow nor flush too early.
fn ldexp(mut x: f64, mut e: i32) -> f64 {
    while e > 1000 {
        x *= 2f64.powi(1000);
        e -= 1000;
    }
    while e < -1000 {
        x *= 2f64.powi(-1000);
        e += 1000;
        if x == 0.0 {
            return x;
        }
    }
    x * 2f64.powi(e)
}

/// Rust's "1.5e3" as C's "1.5e+03".
fn c_exp(s: &str, upper: bool) -> String {
    let (m, e) = s.split_once('e').unwrap_or((s, "0"));
    let e: i32 = e.parse().unwrap_or(0);
    let r = format!("{m}e{}{:02}", if e < 0 { '-' } else { '+' }, e.abs());
    if upper {
        r.to_uppercase()
    } else {
        r
    }
}

/// `%g` without `#`: no trailing zeros after the point.
fn strip_zeros(s: &str) -> String {
    let (m, e) = match s.find(['e', 'E']) {
        Some(k) => (&s[..k], &s[k..]),
        None => (s, ""),
    };
    let m = if m.contains('.') {
        m.trim_end_matches('0').trim_end_matches('.')
    } else {
        m
    };
    format!("{m}{e}")
}

fn exponent(v: f64, digits: usize) -> i32 {
    format!("{:.*e}", digits, v)
        .split_once('e')
        .and_then(|(_, e)| e.parse().ok())
        .unwrap_or(0)
}

/// `|v|` by conversion `conv` (no sign, no padding).
fn body(conv: u8, flags: i32, prec: i32, v: f64) -> String {
    let upper = conv.is_ascii_uppercase();
    if !v.is_finite() {
        let s = if v.is_nan() { "nan" } else { "inf" };
        return if upper { s.to_uppercase() } else { s.into() };
    }
    let v = v.abs();
    let hash = flags & F_HASH != 0;
    match conv.to_ascii_lowercase() {
        b'f' => {
            let s = format!("{:.*}", if prec < 0 { 6 } else { prec as usize }, v);
            if hash && !s.contains('.') {
                s + "."
            } else {
                s
            }
        }
        b'e' => {
            let p = if prec < 0 { 6 } else { prec as usize };
            let s = format!("{:.*e}", p, v);
            let s = if hash && p == 0 {
                s.replacen('e', ".e", 1)
            } else {
                s
            };
            c_exp(&s, upper)
        }
        b'g' => {
            let p = match prec {
                p if p < 0 => 6,
                0 => 1,
                p => p,
            };
            let x = if v == 0.0 {
                0
            } else {
                exponent(v, (p - 1) as usize)
            };
            let s = if p > x && x >= -4 {
                let s = format!("{:.*}", (p - 1 - x) as usize, v);
                if hash && !s.contains('.') {
                    s + "."
                } else {
                    s
                }
            } else {
                c_exp(&format!("{:.*e}", (p - 1) as usize, v), upper)
            };
            if hash {
                s
            } else {
                strip_zeros(&s)
            }
        }
        _ => hexfloat(v, prec, upper),
    }
}

/// `%a`: 0x1.8p+3.
fn hexfloat(v: f64, prec: i32, upper: bool) -> String {
    let s = if v == 0.0 {
        match prec {
            p if p > 0 => format!("0x0.{}p+0", "0".repeat(p as usize)),
            _ => "0x0p+0".into(),
        }
    } else {
        let bits = v.to_bits();
        let raw_e = ((bits >> 52) & 0x7ff) as i32;
        let frac = bits & ((1 << 52) - 1);
        // subnormals as 0x0.xxxp-1022, as C libraries print them
        let (mut lead, mut m, e) = if raw_e == 0 {
            (0u64, frac, -1022)
        } else {
            (1u64, frac, raw_e - 1023)
        };
        let digits = if (0..13).contains(&prec) {
            let shift = 4 * (13 - prec as u32);
            let half = 1u64 << (shift - 1);
            let rest = m & ((1 << shift) - 1);
            m >>= shift;
            // round half to even
            let odd = if prec == 0 { lead & 1 == 1 } else { m & 1 == 1 };
            if rest > half || (rest == half && odd) {
                m += 1;
                if prec == 0 || m >> (4 * prec) != 0 {
                    m &= (1u64 << (4 * prec)).wrapping_sub(1);
                    lead += 1;
                }
            }
            if prec == 0 {
                String::new()
            } else {
                format!("{:0w$x}", m, w = prec as usize)
            }
        } else {
            let mut h = format!("{m:013x}");
            if prec < 0 {
                h = h.trim_end_matches('0').to_string();
            } else {
                h += &"0".repeat(prec as usize - 13);
            }
            h
        };
        let dot = if digits.is_empty() { "" } else { "." };
        format!(
            "0x{lead}{dot}{digits}p{}{}",
            if e < 0 { '-' } else { '+' },
            e.abs()
        )
    };
    if upper {
        s.to_uppercase()
    } else {
        s
    }
}

/// One float conversion of `printf`, as C writes it.
pub fn fmt_float(conv: u8, flags: i32, width: i32, prec: i32, v: f64) -> String {
    let body = body(conv, flags, prec, v);
    let sign = if v.is_sign_negative() && !v.is_nan() {
        "-"
    } else if flags & F_PLUS != 0 {
        "+"
    } else if flags & F_SPACE != 0 {
        " "
    } else {
        ""
    };
    let len = sign.len() + body.len();
    let n = (width.max(0) as usize).saturating_sub(len);
    if n == 0 {
        format!("{sign}{body}")
    } else if flags & F_MINUS != 0 {
        format!("{sign}{body}{}", " ".repeat(n))
    } else if flags & F_ZERO != 0 && v.is_finite() {
        // after the sign and a 0x
        match body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
            Some(rest) => format!("{sign}{}{}{rest}", &body[..2], "0".repeat(n)),
            None => format!("{sign}{}{body}", "0".repeat(n)),
        }
    } else {
        format!("{}{sign}{body}", " ".repeat(n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::{c_char, c_int, CStr, CString};

    extern "C" {
        fn snprintf(buf: *mut c_char, n: usize, fmt: *const c_char, ...) -> c_int;
        #[link_name = "strtod"]
        fn c_strtod(s: *const c_char, end: *mut *mut c_char) -> f64;
    }

    /// The host C library's answer.
    fn c(fmt: &str, v: f64) -> String {
        let f = CString::new(fmt).unwrap();
        let mut buf = vec![0 as c_char; 512];
        unsafe {
            snprintf(buf.as_mut_ptr(), buf.len(), f.as_ptr(), v);
            CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned()
        }
    }

    /// `fmt` (one conversion, nothing else) by `fmt_float`.
    fn ours(fmt: &str, v: f64) -> String {
        let b = fmt.as_bytes();
        let mut i = 1;
        let mut flags = 0;
        loop {
            match b[i] {
                b'-' => flags |= F_MINUS,
                b'+' => flags |= F_PLUS,
                b' ' => flags |= F_SPACE,
                b'#' => flags |= F_HASH,
                b'0' => flags |= F_ZERO,
                _ => break,
            }
            i += 1;
        }
        let mut width = 0;
        while b[i].is_ascii_digit() {
            width = width * 10 + (b[i] - b'0') as i32;
            i += 1;
        }
        let mut prec = -1;
        if b[i] == b'.' {
            i += 1;
            prec = 0;
            while b[i].is_ascii_digit() {
                prec = prec * 10 + (b[i] - b'0') as i32;
                i += 1;
            }
        }
        fmt_float(b[i], flags, width, prec, v) + &fmt[i + 1..]
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn floats_print_as_the_c_library_prints_them() {
        let values = [
            0.0,
            -0.0,
            1.0,
            -1.5,
            0.1,
            1.0 / 3.0,
            2.0 / 3.0,
            123456.789,
            1e20,
            1e-5,
            1e-4,
            9.9999999,
            0.5,
            2.5,
            1e300,
            5e-324,
            2.2250738585072014e-308,
            9007199254740993.0,
            f64::MAX,
            f64::INFINITY,
            f64::NEG_INFINITY,
            100.0,
            1234567.0,
            0.000123456,
            255.255,
        ];
        let fmts = [
            "%f", "%.0f", "%.3f", "%10.2f", "%-10.2f|", "%+f", "% f", "%010.3f", "%#.0f", "%e",
            "%.0e", "%.3E", "%12.4e", "%#.0e", "%g", "%.14g", "%.17g", "%.1g", "%.0g", "%#g", "%G",
            "%10g", "%-12g|", "%+g", "%010g", "%.3g", "%a", "%.3a", "%.0a", "%A", "%.13a", "%.20a",
            "%015a",
        ];
        let mut bad = Vec::new();
        for f in fmts {
            for v in values {
                let (a, b) = (ours(f, v), c(f, v));
                if a != b {
                    bad.push(format!("{f} of {v:e}: {a:?}, C {b:?}"));
                }
            }
        }
        assert!(bad.is_empty(), "{}", bad.join("\n"));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn numbers_read_as_strtod_reads_them() {
        let mut bad = Vec::new();
        for s in [
            "0",
            "12",
            "  12.5e1 ",
            "-3.25",
            "+.5",
            ".5",
            "5.",
            "1e",
            "1e+",
            "1e-3x",
            "0x10",
            "0x1p4",
            "0x1.8p-1",
            "0X.8",
            "0x",
            "-0x1P+2",
            "1e400",
            "1e-400",
            "abc",
            "",
            "- 1",
            "0x1.fffffffffffff8p0",
            "0x123456789abcdef123",
            "123456789012345678901234567890",
            "4.9406564584124654e-324",
            "2.2250738585072011e-308",
            "0x1p-1074",
            "0x1p-1075",
        ] {
            let cs = CString::new(s).unwrap();
            let mut end: *mut c_char = std::ptr::null_mut();
            let want = unsafe { c_strtod(cs.as_ptr(), &mut end) };
            let used = end as usize - cs.as_ptr() as usize;
            let (v, n) = strtod(s.as_bytes());
            if (v.to_bits(), n) != (want.to_bits(), used) {
                bad.push(format!("{s:?}: {v:e} ({n}), C {want:e} ({used})"));
            }
        }
        assert!(bad.is_empty(), "{}", bad.join("\n"));
    }
}
