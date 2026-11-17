//! The C library's functions written in Rust (see `shim/libc.c` for the
//! rest), exported under their C names.
#![allow(clippy::missing_safety_doc)]

use std::alloc::{alloc, dealloc, realloc as grow, Layout};
use std::cell::RefCell;
use std::ffi::{c_char, c_int, CStr};

/// Bytes before each block: its size (C's `free` is not told it).
const HEADER: usize = 16;

fn layout(n: usize) -> Option<Layout> {
    Layout::from_size_align(n.checked_add(HEADER)?, HEADER).ok()
}

#[no_mangle]
pub unsafe extern "C" fn malloc(n: usize) -> *mut u8 {
    let Some(l) = layout(n) else {
        return std::ptr::null_mut();
    };
    let p = alloc(l);
    if p.is_null() {
        return p;
    }
    *(p as *mut usize) = n;
    p.add(HEADER)
}

#[no_mangle]
pub unsafe extern "C" fn free(p: *mut u8) {
    if p.is_null() {
        return;
    }
    let b = p.sub(HEADER);
    let n = *(b as *mut usize);
    dealloc(b, Layout::from_size_align_unchecked(n + HEADER, HEADER));
}

#[no_mangle]
pub unsafe extern "C" fn realloc(p: *mut u8, n: usize) -> *mut u8 {
    if p.is_null() {
        return malloc(n);
    }
    if layout(n).is_none() {
        return std::ptr::null_mut();
    }
    let b = p.sub(HEADER);
    let old = *(b as *mut usize);
    let q = grow(
        b,
        Layout::from_size_align_unchecked(old + HEADER, HEADER),
        n + HEADER,
    );
    if q.is_null() {
        return q;
    }
    *(q as *mut usize) = n;
    q.add(HEADER)
}

macro_rules! math1 {
    ($($n:ident),*) => {$(
        #[no_mangle]
        pub extern "C" fn $n(x: f64) -> f64 {
            libm::$n(x)
        }
    )*};
}
math1!(
    acos, asin, atan, cos, cosh, exp, log, log10, log2, sin, sinh, tan, tanh, floor, ceil, sqrt,
    fabs
);

#[no_mangle]
pub extern "C" fn fmod(a: f64, b: f64) -> f64 {
    libm::fmod(a, b)
}
#[no_mangle]
pub extern "C" fn pow(a: f64, b: f64) -> f64 {
    libm::pow(a, b)
}
#[no_mangle]
pub extern "C" fn atan2(a: f64, b: f64) -> f64 {
    libm::atan2(a, b)
}
#[no_mangle]
pub unsafe extern "C" fn frexp(x: f64, e: *mut c_int) -> f64 {
    let (m, ex) = libm::frexp(x);
    *e = ex;
    m
}
#[no_mangle]
pub extern "C" fn ldexp(x: f64, e: c_int) -> f64 {
    libm::ldexp(x, e)
}

#[no_mangle]
pub unsafe extern "C" fn strtod(s: *const c_char, end: *mut *mut c_char) -> f64 {
    let (v, used) = crate::cfmt::strtod(CStr::from_ptr(s).to_bytes());
    if !end.is_null() {
        *end = s.add(used) as *mut c_char;
    }
    v
}

/// One float conversion of `printf` into `out` (at most `cap` bytes);
/// returns its whole length.
#[no_mangle]
pub unsafe extern "C" fn vblua_fmt_float(
    out: *mut u8,
    cap: c_int,
    conv: c_int,
    flags: c_int,
    width: c_int,
    prec: c_int,
    v: f64,
) -> c_int {
    let s = crate::cfmt::fmt_float(conv as u8, flags, width, prec, v);
    let n = s.len().min(cap.max(0) as usize);
    std::ptr::copy_nonoverlapping(s.as_ptr(), out, n);
    s.len() as c_int
}

thread_local! {
    /// What Lua printed of the current line, per stream (stdout, stderr).
    static LINE: RefCell<[Vec<u8>; 2]> = const { RefCell::new([Vec::new(), Vec::new()]) };
}

/// `print` and Lua's warnings: to the log, a line at a time.
#[no_mangle]
pub unsafe extern "C" fn vblua_write(fd: c_int, s: *const u8, n: usize) {
    let b = std::slice::from_raw_parts(s, n);
    let k = usize::from(fd == 2);
    LINE.with(|l| {
        let mut l = l.borrow_mut();
        for &c in b {
            if c == b'\n' {
                let line = String::from_utf8_lossy(&l[k]).into_owned();
                if k == 0 {
                    log::info!("lua: {line}");
                } else {
                    log::warn!("lua: {line}");
                }
                l[k].clear();
            } else if l[k].len() < 4096 {
                l[k].push(c);
            }
        }
    });
}

#[no_mangle]
pub extern "C" fn vblua_now_ms() -> f64 {
    js_sys::Date::now()
}

thread_local! {
    static START: f64 = js_sys::Date::now();
}

#[no_mangle]
pub extern "C" fn vblua_clock_ms() -> f64 {
    START.with(|s| js_sys::Date::now() - s)
}

/// Local time minus UTC at `t` (seconds since the epoch), in seconds.
#[no_mangle]
pub extern "C" fn vblua_tz_offset_s(t: f64) -> c_int {
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(t * 1000.0));
    (-d.get_timezone_offset() * 60.0) as c_int
}
