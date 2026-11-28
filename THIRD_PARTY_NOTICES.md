# Third-party notices

vibeBMS (`MIT OR Apache-2.0`, see LICENSE-MIT and LICENSE-APACHE) includes or is
derived in part from the works below. The Rust crates it is built from keep their
own licences, listed with their texts in THIRD_PARTY_LICENSES.md (made by
`tools/gen_licenses.sh`; the MPL-2.0 crates symphonia, cssparser, selectors,
dtoa-short and option-ext are used unmodified, source on crates.io).

## Lua 5.4

`crates/vibebms/src/skin/luapat.rs` is a port of Lua's pattern matcher
(`lstrlib.c`); the Lua interpreter itself is linked through mlua, and every
build compiles the lua-src crate's copy of Lua 5.4's sources, unmodified
(lua-src: MIT, Aleksandr Orlenko): mlua on every target but the web, and
`crates/vibebms-lua-web` for the web build (its setjmp / longjmp runtime in
`crates/vibebms-lua-web/shim/libc.c` follows emscripten's and wasi-libc's).

```
Copyright (C) 1994-2026 Lua.org, PUC-Rio.

Permission is hereby granted, free of charge, to any person obtaining a copy of
this software and associated documentation files (the "Software"), to deal in
the Software without restriction, including without limitation the rights to
use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software is furnished to do so,
subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS
FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR
COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER
IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
```

## BeMusicSeeker (unofficial fork), through the bmseeker crate

bmseeker's folder naming patterns, title/artist comparison and chart placement
rules follow the BeMusicSeeker unofficial fork
(https://github.com/Neeted/bemusicseeker-unofficial-fork), MIT:

```
Copyright (c) 2026 ﾏﾝﾊｯﾀﾝｶﾞｯﾌｪ@CyanHowan
Copyright (c) 2023 Sayaka / 黒皇帝
Copyright (c) 2015 ribbit.xyz

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Inter

The embedded Inter typeface is under the SIL Open Font License 1.1
(`crates/vibebms/assets/fonts/OFL.txt`).

## Noto Sans CJK (web build only)

Downloaded by the web build; SIL Open Font License 1.1 (its licence file is
shipped beside it).

## UnRAR (through unrar-rs, Apache-2.0)

```
UnRAR source code may be used in any software to handle RAR archives without limitations free of charge, but cannot be used to develop RAR (WinRAR) compatible archiver and to re-create RAR compression algorithm, which is proprietary. Distribution of modified UnRAR source code in separate form or as a part of other software is permitted, provided that full text of this paragraph, starting from "UnRAR source code" words, is included in license, or in documentation if license is not available, and in source code comments of resulting package.
```
