# Third-party licences

vibeBMS's programs (the desktop app, the Android and iOS apps and the web
build) are built from the Rust crates below, each under its own licence. Each
licence text is here once, then the crates under it, each with the copyright
notices its own licence file has (files that differ only in white space,
letter case, a title line, `http`/`https` or the Apache licence's appendix are
one text). A crate that offers a choice (`MIT OR Apache-2.0`) is listed under
the licence used; its own expression is beside its name. Code vibeBMS took or
derived directly (Lua, the BeMusicSeeker fork, the fonts, UnRAR) is in
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

Made by `tools/gen_licenses.sh` with cargo-about, from Cargo.lock and
about.toml: do not edit it (CI checks that it is current).

505 crates, 28 licence texts (each once, then the crates under it):

- Apache License 2.0 (`Apache-2.0`): 341 crates, 6 texts
- BSD 2-Clause "Simplified" License (`BSD-2-Clause`): 1 crate, 1 text
- BSD 3-Clause "New" or "Revised" License (`BSD-3-Clause`): 4 crates, 3 texts
- Creative Commons Zero v1.0 Universal (`CC0-1.0`): 1 crate, 1 text
- Community Data License Agreement Permissive 2.0 (`CDLA-Permissive-2.0`): 2 crates, 1 text
- ISC License (`ISC`): 6 crates, 4 texts
- MIT License (`MIT`): 118 crates, 6 texts
- Mozilla Public License 2.0 (`MPL-2.0`): 16 crates, 2 texts
- Unicode License v3 (`Unicode-3.0`): 20 crates, 2 texts
- zlib License (`Zlib`): 3 crates, 1 text
- bzip2 and libbzip2 License v1.0.6 (`bzip2-1.0.6`): 1 crate, 1 text

## Apache License 2.0 (`Apache-2.0`)

````text
Apache License
Version 2.0, January 2004
http://www.apache.org/licenses/

TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION

1. Definitions.

"License" shall mean the terms and conditions for use, reproduction, and distribution as defined by Sections 1 through 9 of this document.

"Licensor" shall mean the copyright owner or entity authorized by the copyright owner that is granting the License.

"Legal Entity" shall mean the union of the acting entity and all other entities that control, are controlled by, or are under common control with that entity. For the purposes of this definition, "control" means (i) the power, direct or indirect, to cause the direction or management of such entity, whether by contract or otherwise, or (ii) ownership of fifty percent (50%) or more of the outstanding shares, or (iii) beneficial ownership of such entity.

"You" (or "Your") shall mean an individual or Legal Entity exercising permissions granted by this License.

"Source" form shall mean the preferred form for making modifications, including but not limited to software source code, documentation source, and configuration files.

"Object" form shall mean any form resulting from mechanical transformation or translation of a Source form, including but not limited to compiled object code, generated documentation, and conversions to other media types.

"Work" shall mean the work of authorship, whether in Source or Object form, made available under the License, as indicated by a copyright notice that is included in or attached to the work (an example is provided in the Appendix below).

"Derivative Works" shall mean any work, whether in Source or Object form, that is based on (or derived from) the Work and for which the editorial revisions, annotations, elaborations, or other modifications represent, as a whole, an original work of authorship. For the purposes of this License, Derivative Works shall not include works that remain separable from, or merely link (or bind by name) to the interfaces of, the Work and Derivative Works thereof.

"Contribution" shall mean any work of authorship, including the original version of the Work and any modifications or additions to that Work or Derivative Works thereof, that is intentionally submitted to Licensor for inclusion in the Work by the copyright owner or by an individual or Legal Entity authorized to submit on behalf of the copyright owner. For the purposes of this definition, "submitted" means any form of electronic, verbal, or written communication sent to the Licensor or its representatives, including but not limited to communication on electronic mailing lists, source code control systems, and issue tracking systems that are managed by, or on behalf of, the Licensor for the purpose of discussing and improving the Work, but excluding communication that is conspicuously marked or otherwise designated in writing by the copyright owner as "Not a Contribution."

"Contributor" shall mean Licensor and any individual or Legal Entity on behalf of whom a Contribution has been received by Licensor and subsequently incorporated within the Work.

2. Grant of Copyright License. Subject to the terms and conditions of this License, each Contributor hereby grants to You a perpetual, worldwide, non-exclusive, no-charge, royalty-free, irrevocable copyright license to reproduce, prepare Derivative Works of, publicly display, publicly perform, sublicense, and distribute the Work and such Derivative Works in Source or Object form.

3. Grant of Patent License. Subject to the terms and conditions of this License, each Contributor hereby grants to You a perpetual, worldwide, non-exclusive, no-charge, royalty-free, irrevocable (except as stated in this section) patent license to make, have made, use, offer to sell, sell, import, and otherwise transfer the Work, where such license applies only to those patent claims licensable by such Contributor that are necessarily infringed by their Contribution(s) alone or by combination of their Contribution(s) with the Work to which such Contribution(s) was submitted. If You institute patent litigation against any entity (including a cross-claim or counterclaim in a lawsuit) alleging that the Work or a Contribution incorporated within the Work constitutes direct or contributory patent infringement, then any patent licenses granted to You under this License for that Work shall terminate as of the date such litigation is filed.

4. Redistribution. You may reproduce and distribute copies of the Work or Derivative Works thereof in any medium, with or without modifications, and in Source or Object form, provided that You meet the following conditions:

     (a) You must give any other recipients of the Work or Derivative Works a copy of this License; and

     (b) You must cause any modified files to carry prominent notices stating that You changed the files; and

     (c) You must retain, in the Source form of any Derivative Works that You distribute, all copyright, patent, trademark, and attribution notices from the Source form of the Work, excluding those notices that do not pertain to any part of the Derivative Works; and

     (d) If the Work includes a "NOTICE" text file as part of its distribution, then any Derivative Works that You distribute must include a readable copy of the attribution notices contained within such NOTICE file, excluding those notices that do not pertain to any part of the Derivative Works, in at least one of the following places: within a NOTICE text file distributed as part of the Derivative Works; within the Source form or documentation, if provided along with the Derivative Works; or, within a display generated by the Derivative Works, if and wherever such third-party notices normally appear. The contents of the NOTICE file are for informational purposes only and do not modify the License. You may add Your own attribution notices within Derivative Works that You distribute, alongside or as an addendum to the NOTICE text from the Work, provided that such additional attribution notices cannot be construed as modifying the License.

     You may add Your own copyright statement to Your modifications and may provide additional or different license terms and conditions for use, reproduction, or distribution of Your modifications, or for any such Derivative Works as a whole, provided Your use, reproduction, and distribution of the Work otherwise complies with the conditions stated in this License.

5. Submission of Contributions. Unless You explicitly state otherwise, any Contribution intentionally submitted for inclusion in the Work by You to the Licensor shall be under the terms and conditions of this License, without any additional terms or conditions. Notwithstanding the above, nothing herein shall supersede or modify the terms of any separate license agreement you may have executed with Licensor regarding such Contributions.

6. Trademarks. This License does not grant permission to use the trade names, trademarks, service marks, or product names of the Licensor, except as required for reasonable and customary use in describing the origin of the Work and reproducing the content of the NOTICE file.

7. Disclaimer of Warranty. Unless required by applicable law or agreed to in writing, Licensor provides the Work (and each Contributor provides its Contributions) on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied, including, without limitation, any warranties or conditions of TITLE, NON-INFRINGEMENT, MERCHANTABILITY, or FITNESS FOR A PARTICULAR PURPOSE. You are solely responsible for determining the appropriateness of using or redistributing the Work and assume any risks associated with Your exercise of permissions under this License.

8. Limitation of Liability. In no event and under no legal theory, whether in tort (including negligence), contract, or otherwise, unless required by applicable law (such as deliberate and grossly negligent acts) or agreed to in writing, shall any Contributor be liable to You for damages, including any direct, indirect, special, incidental, or consequential damages of any character arising as a result of this License or out of the use or inability to use the Work (including but not limited to damages for loss of goodwill, work stoppage, computer failure or malfunction, or any and all other commercial damages or losses), even if such Contributor has been advised of the possibility of such damages.

9. Accepting Warranty or Additional Liability. While redistributing the Work or Derivative Works thereof, You may choose to offer, and charge a fee for, acceptance of support, warranty, indemnity, or other liability obligations and/or rights consistent with this License. However, in accepting such obligations, You may act only on Your own behalf and on Your sole responsibility, not on behalf of any other Contributor, and only if You agree to indemnify, defend, and hold each Contributor harmless for any liability incurred by, or claims asserted against, such Contributor by reason of your accepting any such warranty or additional liability.

END OF TERMS AND CONDITIONS

APPENDIX: How to apply the Apache License to your work.

To apply the Apache License to your work, attach the following boilerplate notice, with the fields enclosed by brackets "[]" replaced with your own identifying information. (Don't include the brackets!)  The text should be enclosed in the appropriate comment syntax for the file format. We also recommend that a file or class name and description of purpose be included on the same "printed page" as the copyright notice for easier identification within third-party archives.

Copyright [yyyy] [name of copyright owner]

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
````

- `ab_glyph` (Apache-2.0): https://github.com/alexheretic/ab-glyph; ships no file with this licence's text
- `ab_glyph_rasterizer` (Apache-2.0): https://github.com/alexheretic/ab-glyph; ships no file with this licence's text
- `adler2` (0BSD OR MIT OR Apache-2.0): https://github.com/oyvindln/adler2
- `aes` (MIT OR Apache-2.0): https://github.com/RustCrypto/block-ciphers
- `ahash` (MIT OR Apache-2.0): https://github.com/tkaitchuck/ahash
- `allocator-api2` (MIT OR Apache-2.0): https://github.com/zakarumych/allocator-api2; ships no file with this licence's text
- `alsa` (Apache-2.0 OR MIT): https://github.com/diwic/alsa-rs; ships no file with this licence's text
- `android-activity` (MIT OR Apache-2.0): https://github.com/rust-mobile/android-activity; ships no file with this licence's text
- `android_log-sys` (MIT OR Apache-2.0): https://github.com/rust-mobile/android_log-sys-rs
  - Copyright 2016 The android_log_sys Developers
- `android_logger` (MIT OR Apache-2.0): https://github.com/rust-mobile/android_logger-rs
  - Copyright 2016 The android_logger Developers
- `android_system_properties` (MIT OR Apache-2.0): https://github.com/nical/android_system_properties; ships no file with this licence's text
- `anstream` (MIT OR Apache-2.0): https://github.com/rust-cli/anstyle.git
- `anstyle` (MIT OR Apache-2.0): https://github.com/rust-cli/anstyle.git
- `anstyle-parse` (MIT OR Apache-2.0): https://github.com/rust-cli/anstyle.git
- `anstyle-query` (MIT OR Apache-2.0): https://github.com/rust-cli/anstyle.git
- `anstyle-wincon` (MIT OR Apache-2.0): https://github.com/rust-cli/anstyle.git
- `arrayvec` (MIT OR Apache-2.0): https://github.com/bluss/arrayvec
- `as-raw-xcb-connection` (MIT OR Apache-2.0): https://github.com/psychon/as-raw-xcb-connection
- `autocfg` (Apache-2.0 OR MIT): https://github.com/cuviper/autocfg
- `base64` (MIT OR Apache-2.0): https://github.com/marshallpierce/rust-base64
- `bit-set` (Apache-2.0 OR MIT): https://github.com/contain-rs/bit-set, https://github.com/contain-rs/bit-vec
- `bit-vec` (Apache-2.0 OR MIT): https://github.com/contain-rs/bit-vec
- `bitflags` (MIT OR Apache-2.0): https://github.com/bitflags/bitflags
- `block-buffer` (MIT OR Apache-2.0): https://github.com/RustCrypto/utils
- `block-padding` (MIT OR Apache-2.0): https://github.com/RustCrypto/utils
- `bmseeker` (MIT OR Apache-2.0): https://github.com/vibeBMS/BMSeeker
- `bstr` (MIT OR Apache-2.0): https://github.com/BurntSushi/bstr
- `bumpalo` (MIT OR Apache-2.0): https://github.com/fitzgen/bumpalo
- `bytemuck` (Zlib OR Apache-2.0 OR MIT): https://github.com/Lokathor/bytemuck
- `bytemuck_derive` (Zlib OR Apache-2.0 OR MIT): https://github.com/Lokathor/bytemuck
- `bzip2` (MIT OR Apache-2.0): https://github.com/trifectatechfoundation/bzip2-rs
- `cbc` (MIT OR Apache-2.0): https://github.com/RustCrypto/block-modes
- `cc` (MIT OR Apache-2.0): https://github.com/rust-lang/cc-rs
- `cesu8` (Apache-2.0 OR MIT): https://github.com/emk/cesu8-rs; ships no file with this licence's text
- `cfg-expr` (MIT OR Apache-2.0): https://github.com/EmbarkStudios/cfg-expr
- `cfg-if` (MIT OR Apache-2.0): https://github.com/rust-lang/cfg-if
- `cipher` (MIT OR Apache-2.0): https://github.com/RustCrypto/traits
- `cmov` (Apache-2.0 OR MIT): https://github.com/RustCrypto/utils
- `codespan-reporting` (Apache-2.0): https://github.com/brendanzab/codespan
- `colorchoice` (MIT OR Apache-2.0): https://github.com/rust-cli/anstyle.git
- `concurrent-queue` (Apache-2.0 OR MIT): https://github.com/smol-rs/concurrent-queue
- `console_error_panic_hook` (Apache-2.0 OR MIT): https://github.com/rustwasm/console_error_panic_hook
- `console_log` (MIT OR Apache-2.0): https://github.com/iamcodemaker/console_log
- `const-oid` (Apache-2.0 OR MIT): https://github.com/RustCrypto/formats
- `constant_time_eq` (CC0-1.0 OR MIT-0 OR Apache-2.0): https://codeberg.org/cesarb/constant_time_eq, https://github.com/cesarb/constant_time_eq; ships no file with this licence's text
- `cookie` (MIT OR Apache-2.0): https://github.com/SergioBenitez/cookie-rs
  - Copyright 2017 Sergio Benitez
  - Copyright 2014 Alex Chricton
- `core-foundation` (MIT OR Apache-2.0): https://github.com/servo/core-foundation-rs
- `core-foundation-sys` (MIT OR Apache-2.0): https://github.com/servo/core-foundation-rs
- `core-graphics` (MIT OR Apache-2.0): https://github.com/servo/core-foundation-rs
- `core-graphics-types` (MIT OR Apache-2.0): https://github.com/servo/core-foundation-rs
- `core_detect` (MIT OR Apache-2.0): https://github.com/thomcc/core_detect
- `coreaudio-rs` (MIT OR Apache-2.0): https://github.com/RustAudio/coreaudio-rs.git
- `cpal` (Apache-2.0): https://github.com/RustAudio/cpal
- `cpubits` (MIT OR Apache-2.0): https://github.com/RustCrypto/utils
- `cpufeatures` (MIT OR Apache-2.0): https://github.com/RustCrypto/utils
- `crc-turbo` ((MIT OR Apache-2.0) AND Zlib): https://github.com/scryer-media/crc-turbo
- `crc32fast` (MIT OR Apache-2.0): https://github.com/srijs/rust-crc32fast
- `crossbeam-channel` (MIT OR Apache-2.0): https://github.com/crossbeam-rs/crossbeam
- `crossbeam-deque` (MIT OR Apache-2.0): https://github.com/crossbeam-rs/crossbeam
- `crossbeam-epoch` (MIT OR Apache-2.0): https://github.com/crossbeam-rs/crossbeam
- `crossbeam-utils` (MIT OR Apache-2.0): https://github.com/crossbeam-rs/crossbeam
- `crypto-common` (MIT OR Apache-2.0): https://github.com/RustCrypto/traits
- `ctutils` (Apache-2.0 OR MIT): https://github.com/RustCrypto/utils
- `cursor-icon` (MIT OR Apache-2.0 OR Zlib): https://github.com/rust-windowing/cursor-icon
  - Copyright 2023 Kirill Chibisov
- `dasp_sample` (MIT OR Apache-2.0): https://github.com/rustaudio/sample.git; ships no file with this licence's text
- `delharc` (MIT OR Apache-2.0): https://github.com/royaltm/rust-delharc
  - Copyright 2023 Rafal Michalski
- `deranged` (MIT OR Apache-2.0): https://github.com/jhpratt/deranged
  - Copyright 2024 Jacob Pratt et al.
- `digest` (MIT OR Apache-2.0): https://github.com/RustCrypto/traits
- `dirs` (MIT OR Apache-2.0): https://codeberg.org/dirs/dirs-rs; ships no file with this licence's text
- `dirs-sys` (MIT OR Apache-2.0): https://github.com/dirs-dev/dirs-sys-rs; ships no file with this licence's text
- `dispatch2` (Zlib OR Apache-2.0 OR MIT): https://github.com/madsmtm/objc2; ships no file with this licence's text
- `displaydoc` (MIT OR Apache-2.0): https://github.com/yaahc/displaydoc
- `document-features` (MIT OR Apache-2.0): https://github.com/slint-ui/document-features; ships no file with this licence's text
- `downcast-rs` (MIT OR Apache-2.0): https://github.com/marcianx/downcast-rs
- `dpi` (Apache-2.0 AND MIT): https://github.com/rust-windowing/winit
- `dtoa` (MIT OR Apache-2.0): https://github.com/dtolnay/dtoa; ships no file with this licence's text
- `dunce` (CC0-1.0 OR MIT-0 OR Apache-2.0): https://gitlab.com/kornelski/dunce; ships no file with this licence's text
- `ehttp` (MIT OR Apache-2.0): https://github.com/emilk/ehttp; ships no file with this licence's text
- `either` (MIT OR Apache-2.0): https://github.com/rayon-rs/either
- `encoding_rs` ((Apache-2.0 OR MIT) AND BSD-3-Clause): https://github.com/hsivonen/encoding_rs
- `env_filter` (MIT OR Apache-2.0): https://github.com/rust-cli/env_logger
- `env_logger` (MIT OR Apache-2.0): https://github.com/rust-cli/env_logger
- `equivalent` (Apache-2.0 OR MIT): https://github.com/indexmap-rs/equivalent
- `erased-serde` (MIT OR Apache-2.0): https://github.com/dtolnay/erased-serde; ships no file with this licence's text
- `errno` (MIT OR Apache-2.0): https://github.com/lambda-fairy/rust-errno
- `fastrand` (Apache-2.0 OR MIT): https://github.com/smol-rs/fastrand
- `fdeflate` (MIT OR Apache-2.0): https://github.com/image-rs/fdeflate; ships no file with this licence's text
- `field-offset` (MIT OR Apache-2.0): https://github.com/Diggsey/rust-field-offset; ships no file with this licence's text
- `filetime` (MIT OR Apache-2.0): https://github.com/alexcrichton/filetime
- `find-msvc-tools` (MIT OR Apache-2.0): https://github.com/rust-lang/cc-rs
- `flate2` (MIT OR Apache-2.0): https://github.com/rust-lang/flate2-rs
- `fnv` (Apache-2.0  OR  MIT): https://github.com/servo/rust-fnv
- `foreign-types` (MIT OR Apache-2.0): https://github.com/sfackler/foreign-types
- `foreign-types-macros` (MIT OR Apache-2.0): https://github.com/sfackler/foreign-types
- `foreign-types-shared` (MIT OR Apache-2.0): https://github.com/sfackler/foreign-types
- `form_urlencoded` (MIT OR Apache-2.0): https://github.com/servo/rust-url
- `futures-channel` (MIT OR Apache-2.0): https://github.com/rust-lang/futures-rs
  - Copyright (c) 2016 Alex Crichton
  - Copyright (c) 2017 The Tokio Authors
- `futures-core` (MIT OR Apache-2.0): https://github.com/rust-lang/futures-rs
  - Copyright (c) 2016 Alex Crichton
  - Copyright (c) 2017 The Tokio Authors
- `futures-executor` (MIT OR Apache-2.0): https://github.com/rust-lang/futures-rs
  - Copyright (c) 2016 Alex Crichton
  - Copyright (c) 2017 The Tokio Authors
- `futures-io` (MIT OR Apache-2.0): https://github.com/rust-lang/futures-rs
  - Copyright (c) 2016 Alex Crichton
  - Copyright (c) 2017 The Tokio Authors
- `futures-macro` (MIT OR Apache-2.0): https://github.com/rust-lang/futures-rs
  - Copyright (c) 2016 Alex Crichton
  - Copyright (c) 2017 The Tokio Authors
- `futures-sink` (MIT OR Apache-2.0): https://github.com/rust-lang/futures-rs
  - Copyright (c) 2016 Alex Crichton
  - Copyright (c) 2017 The Tokio Authors
- `futures-task` (MIT OR Apache-2.0): https://github.com/rust-lang/futures-rs
  - Copyright (c) 2016 Alex Crichton
  - Copyright (c) 2017 The Tokio Authors
- `futures-util` (MIT OR Apache-2.0): https://github.com/rust-lang/futures-rs
  - Copyright (c) 2016 Alex Crichton
  - Copyright (c) 2017 The Tokio Authors
- `gethostname` (Apache-2.0): https://codeberg.org/swsnr/gethostname.rs.git
- `getrandom` (MIT OR Apache-2.0): https://github.com/rust-random/getrandom
- `gif` (MIT OR Apache-2.0): https://github.com/image-rs/image-gif
- `gilrs` (Apache-2.0 OR MIT): https://gitlab.com/gilrs-project/gilrs; ships no file with this licence's text
- `gilrs-core` (Apache-2.0 OR MIT): https://gitlab.com/gilrs-project/gilrs; ships no file with this licence's text
- `gl_generator` (Apache-2.0): https://github.com/brendanzab/gl-rs/; ships no file with this licence's text
- `glow` (MIT OR Apache-2.0 OR Zlib): https://github.com/grovesNL/glow
- `glutin_wgl_sys` (Apache-2.0): https://github.com/rust-windowing/glutin
  - Copyright 2022 Kirill Chibisov
- `gpu-allocator` (MIT OR Apache-2.0): https://github.com/Traverse-Research/gpu-allocator
  - Copyright 2021 Traverse Research B.V.
- `half` (MIT OR Apache-2.0): https://github.com/VoidStarKat/half-rs; ships no file with this licence's text
- `hashbrown` (MIT OR Apache-2.0): https://github.com/rust-lang/hashbrown
- `heck` (MIT OR Apache-2.0): https://github.com/withoutboats/heck
- `hmac` (MIT OR Apache-2.0): https://github.com/RustCrypto/MACs
- `html5ever` (MIT OR Apache-2.0): https://github.com/servo/html5ever
- `http` (MIT OR Apache-2.0): https://github.com/hyperium/http
  - Copyright 2017 http-rs authors
- `httparse` (MIT OR Apache-2.0): https://github.com/seanmonstar/httparse
- `hybrid-array` (MIT OR Apache-2.0): https://github.com/RustCrypto/hybrid-array
- `iana-time-zone` (MIT OR Apache-2.0): https://github.com/strawlab/iana-time-zone
  - Copyright 2020 Andrew Straw
- `idna` (MIT OR Apache-2.0): https://github.com/servo/rust-url/
- `idna_adapter` (Apache-2.0 OR MIT): https://github.com/hsivonen/idna_adapter
- `image` (MIT OR Apache-2.0): https://github.com/image-rs/image; ships no file with this licence's text
- `image-webp` (MIT OR Apache-2.0): https://github.com/image-rs/image-webp; ships no file with this licence's text
- `indexmap` (Apache-2.0 OR MIT): https://github.com/indexmap-rs/indexmap
- `inout` (MIT OR Apache-2.0): https://github.com/RustCrypto/utils
- `is_terminal_polyfill` (MIT OR Apache-2.0): https://github.com/polyfill-rs/is_terminal_polyfill
- `itoa` (MIT OR Apache-2.0): https://github.com/dtolnay/itoa; ships no file with this licence's text
- `jni` (MIT OR Apache-2.0): https://github.com/jni-rs/jni-rs; ships no file with this licence's text
- `jni-macros` (MIT OR Apache-2.0): https://github.com/jni-rs/jni-rs; ships no file with this licence's text
- `jni-sys` (MIT OR Apache-2.0): https://github.com/jni-rs/jni-sys
- `jni-sys-macros` (MIT OR Apache-2.0): https://github.com/jni-rs/jni-sys; ships no file with this licence's text
- `jobserver` (MIT OR Apache-2.0): https://github.com/rust-lang/jobserver-rs
- `js-sys` (MIT OR Apache-2.0): https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/js-sys
- `khronos-egl` (MIT OR Apache-2.0): https://github.com/timothee-haudebourg/khronos-egl
- `khronos_api` (Apache-2.0): https://github.com/brendanzab/gl-rs/; ships no file with this licence's text
- `lazy_static` (MIT OR Apache-2.0): https://github.com/rust-lang-nursery/lazy-static.rs
- `libc` (MIT OR Apache-2.0): https://github.com/rust-lang/libc; ships no file with this licence's text
- `linux-raw-sys` (Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT): https://github.com/sunfishcode/linux-raw-sys
- `litrs` (MIT OR Apache-2.0): https://github.com/LukasKalbertodt/litrs; ships no file with this licence's text
- `lock_api` (MIT OR Apache-2.0): https://github.com/Amanieu/parking_lot
- `log` (MIT OR Apache-2.0): https://github.com/rust-lang/log
- `lzma-rust2` (Apache-2.0): https://github.com/hasenbanck/lzma-rust2/
- `mach2` (BSD-2-Clause OR MIT OR Apache-2.0): https://github.com/JohnTitor/mach2; ships no file with this licence's text
- `markup5ever` (MIT OR Apache-2.0): https://github.com/servo/html5ever
- `maybe-dangling` (Zlib OR MIT OR Apache-2.0): https://github.com/danielhenrymantilla/maybe-dangling.rs
  - Copyright 2023 Daniel Henry-Mantilla <daniel.henry.mantilla@gmail.com>
- `md-5` (MIT OR Apache-2.0): https://github.com/RustCrypto/hashes
- `memmap2` (MIT OR Apache-2.0): https://github.com/RazrFalcon/memmap2-rs
  - Copyright [2015] [Dan Burkert]
- `miniz_oxide` (MIT OR Zlib OR Apache-2.0): https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide; ships no file with this licence's text
- `moxcms` (BSD-3-Clause OR Apache-2.0): https://github.com/awxkee/moxcms.git
  - Copyright 2024 Radzivon Bartoshyk
- `multiversion_no_op` (Apache-2.0 OR MIT): https://github.com/hsivonen/multiversion_no_op
- `naga` (MIT OR Apache-2.0): https://github.com/gfx-rs/wgpu; ships no file with this licence's text
- `naga-types` (MIT OR Apache-2.0): https://github.com/gfx-rs/wgpu; ships no file with this licence's text
- `ndk` (MIT OR Apache-2.0): https://github.com/rust-mobile/ndk; ships no file with this licence's text
- `ndk-context` (MIT OR Apache-2.0): https://github.com/rust-windowing/android-ndk-rs; ships no file with this licence's text
- `ndk-sys` (MIT OR Apache-2.0): https://github.com/rust-mobile/ndk; ships no file with this licence's text
- `num-complex` (MIT OR Apache-2.0): https://github.com/rust-num/num-complex
- `num-conv` (MIT OR Apache-2.0): https://github.com/jhpratt/num-conv; ships no file with this licence's text
- `num-derive` (MIT OR Apache-2.0): https://github.com/rust-num/num-derive
- `num-integer` (MIT OR Apache-2.0): https://github.com/rust-num/num-integer
- `num-traits` (MIT OR Apache-2.0): https://github.com/rust-num/num-traits
- `num_enum` (BSD-3-Clause OR MIT OR Apache-2.0): https://github.com/illicitonion/num_enum; ships no file with this licence's text
- `num_enum_derive` (BSD-3-Clause OR MIT OR Apache-2.0): https://github.com/illicitonion/num_enum; ships no file with this licence's text
- `objc2-app-kit` (Zlib OR Apache-2.0 OR MIT): https://github.com/madsmtm/objc2; ships no file with this licence's text
- `objc2-audio-toolbox` (Zlib OR Apache-2.0 OR MIT): https://github.com/madsmtm/objc2; ships no file with this licence's text
- `objc2-avf-audio` (Zlib OR Apache-2.0 OR MIT): https://github.com/madsmtm/objc2; ships no file with this licence's text
- `objc2-core-audio` (Zlib OR Apache-2.0 OR MIT): https://github.com/madsmtm/objc2; ships no file with this licence's text
- `objc2-core-audio-types` (Zlib OR Apache-2.0 OR MIT): https://github.com/madsmtm/objc2; ships no file with this licence's text
- `objc2-core-foundation` (Zlib OR Apache-2.0 OR MIT): https://github.com/madsmtm/objc2; ships no file with this licence's text
- `objc2-core-graphics` (Zlib OR Apache-2.0 OR MIT): https://github.com/madsmtm/objc2; ships no file with this licence's text
- `objc2-exception-helper` (Zlib OR Apache-2.0 OR MIT): https://github.com/madsmtm/objc2; ships no file with this licence's text
- `objc2-game-controller` (Zlib OR Apache-2.0 OR MIT): https://github.com/madsmtm/objc2; ships no file with this licence's text
- `objc2-io-kit` (Zlib OR Apache-2.0 OR MIT): https://github.com/madsmtm/objc2; ships no file with this licence's text
- `objc2-metal` (Zlib OR Apache-2.0 OR MIT): https://github.com/madsmtm/objc2; ships no file with this licence's text
- `objc2-quartz-core` (Zlib OR Apache-2.0 OR MIT): https://github.com/madsmtm/objc2; ships no file with this licence's text
- `objc2-ui-kit` (Zlib OR Apache-2.0 OR MIT): https://github.com/madsmtm/objc2; ships no file with this licence's text
- `objc2-uniform-type-identifiers` (Zlib OR Apache-2.0 OR MIT): https://github.com/madsmtm/objc2; ships no file with this licence's text
- `objc2-web-kit` (Zlib OR Apache-2.0 OR MIT): https://github.com/madsmtm/objc2; ships no file with this licence's text
- `once_cell` (MIT OR Apache-2.0): https://github.com/matklad/once_cell
- `once_cell_polyfill` (MIT OR Apache-2.0): https://github.com/polyfill-rs/once_cell_polyfill
- `openssl-probe` (MIT OR Apache-2.0): https://github.com/rustls/openssl-probe
- `owned_ttf_parser` (Apache-2.0): https://github.com/alexheretic/owned-ttf-parser; ships no file with this licence's text
- `parking_lot` (MIT OR Apache-2.0): https://github.com/Amanieu/parking_lot
- `parking_lot_core` (MIT OR Apache-2.0): https://github.com/Amanieu/parking_lot
- `percent-encoding` (MIT OR Apache-2.0): https://github.com/servo/rust-url/
- `pin-project` (Apache-2.0 OR MIT): https://github.com/taiki-e/pin-project; ships no file with this licence's text
- `pin-project-internal` (Apache-2.0 OR MIT): https://github.com/taiki-e/pin-project; ships no file with this licence's text
- `pin-project-lite` (Apache-2.0 OR MIT): https://github.com/taiki-e/pin-project-lite; ships no file with this licence's text
- `pkg-config` (MIT OR Apache-2.0): https://github.com/rust-lang/pkg-config-rs
- `png` (MIT OR Apache-2.0): https://github.com/image-rs/image-png
- `polling` (Apache-2.0 OR MIT): https://github.com/smol-rs/polling
- `pollster` (Apache-2.0 OR MIT): https://github.com/zesterer/pollster
- `powerfmt` (MIT OR Apache-2.0): https://github.com/jhpratt/powerfmt
  - Copyright 2023 Jacob Pratt et al.
- `ppmd-turbo` (Apache-2.0): https://github.com/scryer-media/ppmd-turbo
- `presser` (MIT OR Apache-2.0): https://github.com/EmbarkStudios/presser
- `proc-macro-crate` (MIT OR Apache-2.0): https://github.com/bkchr/proc-macro-crate
- `proc-macro-error` (MIT OR Apache-2.0): https://gitlab.com/CreepySkeleton/proc-macro-error
  - Copyright 2019-2020 CreepySkeleton <creepy-skeleton@yandex.ru>
- `proc-macro-error-attr` (MIT OR Apache-2.0): https://gitlab.com/CreepySkeleton/proc-macro-error
  - Copyright 2019-2020 CreepySkeleton <creepy-skeleton@yandex.ru>
- `proc-macro2` (MIT OR Apache-2.0): https://github.com/dtolnay/proc-macro2; ships no file with this licence's text
- `profiling` (MIT OR Apache-2.0): https://github.com/aclysma/profiling; ships no file with this licence's text
- `pxfm` (BSD-3-Clause OR Apache-2.0): https://github.com/awxkee/pxfm
  - Copyright 2024 Radzivon Bartoshyk
- `quick-error` (MIT OR Apache-2.0): http://github.com/tailhook/quick-error
- `quote` (MIT OR Apache-2.0): https://github.com/dtolnay/quote; ships no file with this licence's text
- `range-alloc` (MIT OR Apache-2.0): https://github.com/gfx-rs/range-alloc; ships no file with this licence's text
- `raw-window-handle` (MIT OR Apache-2.0 OR Zlib): https://github.com/rust-windowing/raw-window-handle; ships no file with this licence's text
- `raw-window-metal` (MIT OR Apache-2.0): https://github.com/rust-windowing/raw-window-metal
- `rayon` (MIT OR Apache-2.0): https://github.com/rayon-rs/rayon
- `rayon-core` (MIT OR Apache-2.0): https://github.com/rayon-rs/rayon
- `reedsolomon-rs` (Apache-2.0): https://github.com/scryer-media/rarpar
- `regex` (MIT OR Apache-2.0): https://github.com/rust-lang/regex
- `regex-automata` (MIT OR Apache-2.0): https://github.com/rust-lang/regex
- `regex-lite` (MIT OR Apache-2.0): https://github.com/rust-lang/regex
- `regex-syntax` (MIT OR Apache-2.0): https://github.com/rust-lang/regex
- `renderdoc-sys` (MIT OR Apache-2.0): https://github.com/ebkalderon/renderdoc-rs
- `roxmltree` (MIT OR Apache-2.0): https://github.com/RazrFalcon/roxmltree
- `rustc-hash` (Apache-2.0 OR MIT): https://github.com/rust-lang-nursery/rustc-hash, https://github.com/rust-lang/rustc-hash; ships no file with this licence's text
- `rustc_version` (MIT OR Apache-2.0): https://github.com/djc/rustc-version-rs
- `rustix` (Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT): https://github.com/bytecodealliance/rustix
- `rustls` (Apache-2.0 OR ISC OR MIT): https://github.com/rustls/rustls
- `rustls-native-certs` (Apache-2.0 OR ISC OR MIT): https://github.com/rustls/rustls-native-certs
- `rustls-pki-types` (MIT OR Apache-2.0): https://github.com/rustls/pki-types
  - Copyright 2023 Dirkjan Ochtman
- `rustls-platform-verifier` (MIT OR Apache-2.0): https://github.com/rustls/rustls-platform-verifier
- `rustls-platform-verifier-android` (MIT OR Apache-2.0): https://github.com/rustls/rustls-platform-verifier; ships no file with this licence's text
- `rustversion` (MIT OR Apache-2.0): https://github.com/dtolnay/rustversion; ships no file with this licence's text
- `scoped-tls` (MIT OR Apache-2.0): https://github.com/alexcrichton/scoped-tls
- `scopeguard` (MIT OR Apache-2.0): https://github.com/bluss/scopeguard
- `security-framework` (MIT OR Apache-2.0): https://github.com/kornelski/rust-security-framework
- `security-framework-sys` (MIT OR Apache-2.0): https://github.com/kornelski/rust-security-framework
- `self-replace` (Apache-2.0): https://github.com/mitsuhiko/self-replace
- `semver` (MIT OR Apache-2.0): https://github.com/dtolnay/semver; ships no file with this licence's text
- `serde` (MIT OR Apache-2.0): https://github.com/serde-rs/serde; ships no file with this licence's text
- `serde_core` (MIT OR Apache-2.0): https://github.com/serde-rs/serde; ships no file with this licence's text
- `serde_derive` (MIT OR Apache-2.0): https://github.com/serde-rs/serde; ships no file with this licence's text
- `serde_json` (MIT OR Apache-2.0): https://github.com/serde-rs/json; ships no file with this licence's text
- `serde_spanned` (MIT OR Apache-2.0): https://github.com/toml-rs/toml
  - Copyright Individual contributors
- `servo_arc` (MIT OR Apache-2.0): https://github.com/servo/stylo
- `sevenz-rust2` (Apache-2.0): https://github.com/hasenbanck/sevenz-rust
- `sha2` (MIT OR Apache-2.0): https://github.com/RustCrypto/hashes
- `shlex` (MIT OR Apache-2.0): https://github.com/comex/rust-shlex; ships no file with this licence's text
- `simd_cesu8` (Apache-2.0 OR MIT): https://github.com/seancroach/simd_cesu8
- `simdutf8` (MIT OR Apache-2.0): https://github.com/rusticstuff/simdutf8; ships no file with this licence's text
- `siphasher` (MIT OR Apache-2.0): https://github.com/jedisct1/rust-siphash
- `smallvec` (MIT OR Apache-2.0): https://github.com/servo/rust-smallvec
- `smol_str` (MIT OR Apache-2.0): https://github.com/rust-analyzer/smol_str
- `spirv` (Apache-2.0): https://github.com/gfx-rs/rspirv; ships no file with this licence's text
- `stable_deref_trait` (MIT OR Apache-2.0): https://github.com/storyyeller/stable_deref_trait
- `static_assertions` (MIT OR Apache-2.0): https://github.com/nvzqz/static-assertions-rs
- `string_cache` (MIT OR Apache-2.0): https://github.com/servo/string-cache
- `string_cache_codegen` (MIT OR Apache-2.0): https://github.com/servo/string-cache
- `syn` (MIT OR Apache-2.0): https://github.com/dtolnay/syn; ships no file with this licence's text
- `sys-locale` (MIT OR Apache-2.0): https://github.com/1Password/sys-locale
- `system-deps` (MIT OR Apache-2.0): https://github.com/gdesmott/system-deps
- `tao-macros` (MIT OR Apache-2.0): https://github.com/tauri-apps/tao
- `tar` (MIT OR Apache-2.0): https://github.com/composefs/tar-rs
- `tempfile` (MIT OR Apache-2.0): https://github.com/Stebalien/tempfile
- `tendril` (MIT OR Apache-2.0): https://github.com/servo/html5ever
- `thiserror` (MIT OR Apache-2.0): https://github.com/dtolnay/thiserror; ships no file with this licence's text
- `thiserror-impl` (MIT OR Apache-2.0): https://github.com/dtolnay/thiserror; ships no file with this licence's text
- `time` (MIT OR Apache-2.0): https://github.com/time-rs/time; ships no file with this licence's text
- `time-core` (MIT OR Apache-2.0): https://github.com/time-rs/time; ships no file with this licence's text
- `time-macros` (MIT OR Apache-2.0): https://github.com/time-rs/time; ships no file with this licence's text
- `tinyvec` (Zlib OR Apache-2.0 OR MIT): https://github.com/Lokathor/tinyvec
- `toml` (MIT OR Apache-2.0): https://github.com/toml-rs/toml
  - Copyright Individual contributors
- `toml_datetime` (MIT OR Apache-2.0): https://github.com/toml-rs/toml
  - Copyright Individual contributors
- `toml_edit` (MIT OR Apache-2.0): https://github.com/toml-rs/toml
  - Copyright Individual contributors
- `toml_parser` (MIT OR Apache-2.0): https://github.com/toml-rs/toml
  - Copyright Individual contributors
- `toml_writer` (MIT OR Apache-2.0): https://github.com/toml-rs/toml
  - Copyright Individual contributors
- `ttf-parser` (MIT OR Apache-2.0): https://github.com/harfbuzz/ttf-parser
- `typed-path` (MIT OR Apache-2.0): https://github.com/chipsenkbeil/typed-path; ships no file with this licence's text
- `typeid` (MIT OR Apache-2.0): https://github.com/dtolnay/typeid; ships no file with this licence's text
- `typenum` (MIT OR Apache-2.0): https://github.com/paholg/typenum
  - Copyright 2014 Paho Lurie-Gregg
- `ucd-trie` (MIT OR Apache-2.0): https://github.com/BurntSushi/ucd-generate
- `unicode-ident` ((MIT OR Apache-2.0) AND Unicode-3.0): https://github.com/dtolnay/unicode-ident; ships no file with this licence's text
- `unicode-normalization` ((MIT OR Apache-2.0) AND Unicode-3.0): https://github.com/unicode-rs/unicode-normalization
- `unicode-segmentation` (MIT OR Apache-2.0): https://github.com/unicode-rs/unicode-segmentation
- `unicode-width` (MIT OR Apache-2.0): https://github.com/unicode-rs/unicode-width
- `ureq` (MIT OR Apache-2.0): https://github.com/algesten/ureq
- `ureq-proto` (MIT OR Apache-2.0): https://github.com/algesten/ureq-proto
- `url` (MIT OR Apache-2.0): https://github.com/servo/rust-url
- `utf8-zero` (MIT OR Apache-2.0): https://github.com/algesten/utf8-zero; ships no file with this licence's text
- `utf8_iter` (Apache-2.0 OR MIT): https://github.com/hsivonen/utf8_iter
- `utf8parse` (Apache-2.0 OR MIT): https://github.com/alacritty/vte; ships no file with this licence's text
- `utf_types` (Apache-2.0 OR MIT): https://github.com/hsivonen/utf_types
- `uuid` (Apache-2.0 OR MIT): https://github.com/uuid-rs/uuid
- `vec_map` (MIT OR Apache-2.0): https://github.com/contain-rs/vec-map
- `version_check` (MIT OR Apache-2.0): https://github.com/SergioBenitez/version_check
- `wasm-bindgen` (MIT OR Apache-2.0): https://github.com/wasm-bindgen/wasm-bindgen
- `wasm-bindgen-futures` (MIT OR Apache-2.0): https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/futures
- `wasm-bindgen-macro` (MIT OR Apache-2.0): https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/macro
- `wasm-bindgen-macro-support` (MIT OR Apache-2.0): https://github.com/wasm-bindgen/wasm-bindgen/tree/main/crates/macro-support
- `wasm-bindgen-shared` (MIT OR Apache-2.0): https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/shared
- `wasm-streams` (MIT OR Apache-2.0): https://github.com/MattiasBuelens/wasm-streams/; ships no file with this licence's text
- `web-sys` (MIT OR Apache-2.0): https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/web-sys
- `web-time` (MIT OR Apache-2.0): https://github.com/daxpedda/web-time
  - Copyright 2023 dAxpeDDa
- `web_atoms` (MIT OR Apache-2.0): https://github.com/servo/html5ever
- `webbrowser` (MIT OR Apache-2.0): https://github.com/amodm/webbrowser-rs
  - Copyright (c) 2015-2022 Amod Malviya
- `weezl` (MIT OR Apache-2.0): https://github.com/image-rs/weezl
- `wgpu` (MIT OR Apache-2.0): https://github.com/gfx-rs/wgpu; ships no file with this licence's text
- `wgpu-core` (MIT OR Apache-2.0): https://github.com/gfx-rs/wgpu; ships no file with this licence's text
- `wgpu-core-deps-apple` (MIT OR Apache-2.0): https://github.com/gfx-rs/wgpu; ships no file with this licence's text
- `wgpu-core-deps-wasm` (MIT OR Apache-2.0): https://github.com/gfx-rs/wgpu; ships no file with this licence's text
- `wgpu-core-deps-windows-linux-android` (MIT OR Apache-2.0): https://github.com/gfx-rs/wgpu; ships no file with this licence's text
- `wgpu-hal` (MIT OR Apache-2.0): https://github.com/gfx-rs/wgpu; ships no file with this licence's text
- `wgpu-naga-bridge` (MIT OR Apache-2.0): https://github.com/gfx-rs/wgpu; ships no file with this licence's text
- `wgpu-types` (MIT OR Apache-2.0): https://github.com/gfx-rs/wgpu; ships no file with this licence's text
- `winapi` (MIT OR Apache-2.0): https://github.com/retep998/winapi-rs
- `windows` (MIT OR Apache-2.0): https://github.com/microsoft/windows-rs
  - Copyright (c) Microsoft Corporation.
- `windows-collections` (MIT OR Apache-2.0): https://github.com/microsoft/windows-rs
  - Copyright (c) Microsoft Corporation.
- `windows-core` (MIT OR Apache-2.0): https://github.com/microsoft/windows-rs
  - Copyright (c) Microsoft Corporation.
- `windows-future` (MIT OR Apache-2.0): https://github.com/microsoft/windows-rs
  - Copyright (c) Microsoft Corporation.
- `windows-implement` (MIT OR Apache-2.0): https://github.com/microsoft/windows-rs
  - Copyright (c) Microsoft Corporation.
- `windows-interface` (MIT OR Apache-2.0): https://github.com/microsoft/windows-rs
  - Copyright (c) Microsoft Corporation.
- `windows-link` (MIT OR Apache-2.0): https://github.com/microsoft/windows-rs
  - Copyright (c) Microsoft Corporation.
- `windows-numerics` (MIT OR Apache-2.0): https://github.com/microsoft/windows-rs
  - Copyright (c) Microsoft Corporation.
- `windows-result` (MIT OR Apache-2.0): https://github.com/microsoft/windows-rs
  - Copyright (c) Microsoft Corporation.
- `windows-strings` (MIT OR Apache-2.0): https://github.com/microsoft/windows-rs
  - Copyright (c) Microsoft Corporation.
- `windows-sys` (MIT OR Apache-2.0): https://github.com/microsoft/windows-rs
  - Copyright (c) Microsoft Corporation.
- `windows-targets` (MIT OR Apache-2.0): https://github.com/microsoft/windows-rs
  - Copyright (c) Microsoft Corporation.
- `windows-threading` (MIT OR Apache-2.0): https://github.com/microsoft/windows-rs
  - Copyright (c) Microsoft Corporation.
- `windows-version` (MIT OR Apache-2.0): https://github.com/microsoft/windows-rs
  - Copyright (c) Microsoft Corporation.
- `windows_aarch64_msvc` (MIT OR Apache-2.0): https://github.com/microsoft/windows-rs
  - Copyright (c) Microsoft Corporation.
- `windows_x86_64_gnu` (MIT OR Apache-2.0): https://github.com/microsoft/windows-rs
  - Copyright (c) Microsoft Corporation.
- `windows_x86_64_msvc` (MIT OR Apache-2.0): https://github.com/microsoft/windows-rs
  - Copyright (c) Microsoft Corporation.
- `winit` (Apache-2.0): https://github.com/rust-windowing/winit
- `wry` (Apache-2.0 OR MIT): https://github.com/tauri-apps/wry
- `x11rb` (MIT OR Apache-2.0): https://github.com/psychon/x11rb
- `x11rb-protocol` (MIT OR Apache-2.0): https://github.com/psychon/x11rb
- `xkeysym` (MIT OR Apache-2.0 OR Zlib): https://github.com/notgull/xkeysym
  - Copyright 2022-2023 John Nunley
- `zerocopy` (BSD-2-Clause OR Apache-2.0 OR MIT): https://github.com/google/zerocopy
  - Copyright 2023 The Fuchsia Authors
- `zerocopy-derive` (BSD-2-Clause OR Apache-2.0 OR MIT): https://github.com/google/zerocopy
  - Copyright 2023 The Fuchsia Authors
- `zeroize` (Apache-2.0 OR MIT): https://github.com/RustCrypto/utils
- `zeroize_derive` (Apache-2.0 OR MIT): https://github.com/RustCrypto/utils
- `zune-core` (MIT OR Apache-2.0 OR Zlib): https://github.com/etemesi254/zune-image
- `zune-jpeg` (MIT OR Apache-2.0 OR Zlib): https://github.com/etemesi254/zune-image/tree/dev/crates/zune-jpeg

## Apache License 2.0 (`Apache-2.0`)

````text
Apache License

Version 2.0, January 2004

http://www.apache.org/licenses/

TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION

1. Definitions.

"License" shall mean the terms and conditions for use, reproduction, and distribution as defined by Sections 1 through 9 of this document.

"Licensor" shall mean the copyright owner or entity authorized by the copyright owner that is granting the License.

"Legal Entity" shall mean the union of the acting entity and all other entities that control, are controlled by, or are under common control with that entity. For the purposes of this definition, "control" means (i) the power, direct or indirect, to cause the direction or management of such entity, whether by contract or otherwise, or (ii) ownership of fifty percent (50%) or more of the outstanding shares, or (iii) beneficial ownership of such entity.

"You" (or "Your") shall mean an individual or Legal Entity exercising permissions granted by this License.

"Source" form shall mean the preferred form for making modifications, including but not limited to software source code, documentation source, and configuration files.

"Object" form shall mean any form resulting from mechanical transformation or translation of a Source form, including but not limited to compiled object code, generated documentation, and conversions to other media types.

"Work" shall mean the work of authorship, whether in Source or Object form, made available under the License, as indicated by a copyright notice that is included in or attached to the work (an example is provided in the Appendix below).

"Derivative Works" shall mean any work, whether in Source or Object form, that is based on (or derived from) the Work and for which the editorial revisions, annotations, elaborations, or other modifications represent, as a whole, an original work of authorship. For the purposes of this License, Derivative Works shall not include works that remain separable from, or merely link (or bind by name) to the interfaces of, the Work and Derivative Works thereof.

"Contribution" shall mean any work of authorship, including the original version of the Work and any modifications or additions to that Work or Derivative Works thereof, that is intentionally submitted to Licensor for inclusion in the Work by the copyright owner or by an individual or Legal Entity authorized to submit on behalf of the copyright owner. For the purposes of this definition, "submitted" means any form of electronic, verbal, or written communication sent to the Licensor or its representatives, including but not limited to communication on electronic mailing lists, source code control systems, and issue tracking systems that are managed by, or on behalf of, the Licensor for the purpose of discussing and improving the Work, but excluding communication that is conspicuously marked or otherwise designated in writing by the copyright owner as "Not a Contribution."

"Contributor" shall mean Licensor and any individual or Legal Entity on behalf of whom a Contribution has been received by Licensor and subsequently incorporated within the Work.

2. Grant of Copyright License. Subject to the terms and conditions of this License, each Contributor hereby grants to You a perpetual, worldwide, non-exclusive, no-charge, royalty-free, irrevocable copyright license to reproduce, prepare Derivative Works of, publicly display, publicly perform, sublicense, and distribute the Work and such Derivative Works in Source or Object form.

3. Grant of Patent License. Subject to the terms and conditions of this License, each Contributor hereby grants to You a perpetual, worldwide, non-exclusive, no-charge, royalty-free, irrevocable (except as stated in this section) patent license to make, have made, use, offer to sell, sell, import, and otherwise transfer the Work, where such license applies only to those patent claims licensable by such Contributor that are necessarily infringed by their Contribution(s) alone or by combination of their Contribution(s) with the Work to which such Contribution(s) was submitted. If You institute patent litigation against any entity (including a cross-claim or counterclaim in a lawsuit) alleging that the Work or a Contribution incorporated within the Work constitutes direct or contributory patent infringement, then any patent licenses granted to You under this License for that Work shall terminate as of the date such litigation is filed.

4. Redistribution. You may reproduce and distribute copies of the Work or Derivative Works thereof in any medium, with or without modifications, and in Source or Object form, provided that You meet the following conditions:

You must give any other recipients of the Work or Derivative Works a copy of this License; and
You must cause any modified files to carry prominent notices stating that You changed the files; and
You must retain, in the Source form of any Derivative Works that You distribute, all copyright, patent, trademark, and attribution notices from the Source form of the Work, excluding those notices that do not pertain to any part of the Derivative Works; and
If the Work includes a "NOTICE" text file as part of its distribution, then any Derivative Works that You distribute must include a readable copy of the attribution notices contained within such NOTICE file, excluding those notices that do not pertain to any part of the Derivative Works, in at least one of the following places: within a NOTICE text file distributed as part of the Derivative Works; within the Source form or documentation, if provided along with the Derivative Works; or, within a display generated by the Derivative Works, if and wherever such third-party notices normally appear. The contents of the NOTICE file are for informational purposes only and do not modify the License. You may add Your own attribution notices within Derivative Works that You distribute, alongside or as an addendum to the NOTICE text from the Work, provided that such additional attribution notices cannot be construed as modifying the License.

You may add Your own copyright statement to Your modifications and may provide additional or different license terms and conditions for use, reproduction, or distribution of Your modifications, or for any such Derivative Works as a whole, provided Your use, reproduction, and distribution of the Work otherwise complies with the conditions stated in this License.
5. Submission of Contributions. Unless You explicitly state otherwise, any Contribution intentionally submitted for inclusion in the Work by You to the Licensor shall be under the terms and conditions of this License, without any additional terms or conditions. Notwithstanding the above, nothing herein shall supersede or modify the terms of any separate license agreement you may have executed with Licensor regarding such Contributions.

6. Trademarks. This License does not grant permission to use the trade names, trademarks, service marks, or product names of the Licensor, except as required for reasonable and customary use in describing the origin of the Work and reproducing the content of the NOTICE file.

7. Disclaimer of Warranty. Unless required by applicable law or agreed to in writing, Licensor provides the Work (and each Contributor provides its Contributions) on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied, including, without limitation, any warranties or conditions of TITLE, NON-INFRINGEMENT, MERCHANTABILITY, or FITNESS FOR A PARTICULAR PURPOSE. You are solely responsible for determining the appropriateness of using or redistributing the Work and assume any risks associated with Your exercise of permissions under this License.

8. Limitation of Liability. In no event and under no legal theory, whether in tort (including negligence), contract, or otherwise, unless required by applicable law (such as deliberate and grossly negligent acts) or agreed to in writing, shall any Contributor be liable to You for damages, including any direct, indirect, special, incidental, or consequential damages of any character arising as a result of this License or out of the use or inability to use the Work (including but not limited to damages for loss of goodwill, work stoppage, computer failure or malfunction, or any and all other commercial damages or losses), even if such Contributor has been advised of the possibility of such damages.

9. Accepting Warranty or Additional Liability. While redistributing the Work or Derivative Works thereof, You may choose to offer, and charge a fee for, acceptance of support, warranty, indemnity, or other liability obligations and/or rights consistent with this License. However, in accepting such obligations, You may act only on Your own behalf and on Your sole responsibility, not on behalf of any other Contributor, and only if You agree to indemnify, defend, and hold each Contributor harmless for any liability incurred by, or claims asserted against, such Contributor by reason of your accepting any such warranty or additional liability.

END OF TERMS AND CONDITIONS

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
````

- `ash` (MIT OR Apache-2.0): https://github.com/ash-rs/ash
  - Copyright 2016 Maik Klein

## Apache License 2.0 (`Apache-2.0`)

````text
Rust-chrono is dual-licensed under The MIT License [1] and
Apache 2.0 License [2]. Copyright (c) 2014--2026, Kang Seonghoon and
contributors.

Nota Bene: This is same as the Rust Project's own license.

[1]: <http://opensource.org/licenses/MIT>, which is reproduced below:

~~~~
The MIT License (MIT)

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
~~~~

[2]: <http://www.apache.org/licenses/LICENSE-2.0>, which is reproduced below:

~~~~
                              Apache License
                        Version 2.0, January 2004
                     http://www.apache.org/licenses/

TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION

1. Definitions.

   "License" shall mean the terms and conditions for use, reproduction,
   and distribution as defined by Sections 1 through 9 of this document.

   "Licensor" shall mean the copyright owner or entity authorized by
   the copyright owner that is granting the License.

   "Legal Entity" shall mean the union of the acting entity and all
   other entities that control, are controlled by, or are under common
   control with that entity. For the purposes of this definition,
   "control" means (i) the power, direct or indirect, to cause the
   direction or management of such entity, whether by contract or
   otherwise, or (ii) ownership of fifty percent (50%) or more of the
   outstanding shares, or (iii) beneficial ownership of such entity.

   "You" (or "Your") shall mean an individual or Legal Entity
   exercising permissions granted by this License.

   "Source" form shall mean the preferred form for making modifications,
   including but not limited to software source code, documentation
   source, and configuration files.

   "Object" form shall mean any form resulting from mechanical
   transformation or translation of a Source form, including but
   not limited to compiled object code, generated documentation,
   and conversions to other media types.

   "Work" shall mean the work of authorship, whether in Source or
   Object form, made available under the License, as indicated by a
   copyright notice that is included in or attached to the work
   (an example is provided in the Appendix below).

   "Derivative Works" shall mean any work, whether in Source or Object
   form, that is based on (or derived from) the Work and for which the
   editorial revisions, annotations, elaborations, or other modifications
   represent, as a whole, an original work of authorship. For the purposes
   of this License, Derivative Works shall not include works that remain
   separable from, or merely link (or bind by name) to the interfaces of,
   the Work and Derivative Works thereof.

   "Contribution" shall mean any work of authorship, including
   the original version of the Work and any modifications or additions
   to that Work or Derivative Works thereof, that is intentionally
   submitted to Licensor for inclusion in the Work by the copyright owner
   or by an individual or Legal Entity authorized to submit on behalf of
   the copyright owner. For the purposes of this definition, "submitted"
   means any form of electronic, verbal, or written communication sent
   to the Licensor or its representatives, including but not limited to
   communication on electronic mailing lists, source code control systems,
   and issue tracking systems that are managed by, or on behalf of, the
   Licensor for the purpose of discussing and improving the Work, but
   excluding communication that is conspicuously marked or otherwise
   designated in writing by the copyright owner as "Not a Contribution."

   "Contributor" shall mean Licensor and any individual or Legal Entity
   on behalf of whom a Contribution has been received by Licensor and
   subsequently incorporated within the Work.

2. Grant of Copyright License. Subject to the terms and conditions of
   this License, each Contributor hereby grants to You a perpetual,
   worldwide, non-exclusive, no-charge, royalty-free, irrevocable
   copyright license to reproduce, prepare Derivative Works of,
   publicly display, publicly perform, sublicense, and distribute the
   Work and such Derivative Works in Source or Object form.

3. Grant of Patent License. Subject to the terms and conditions of
   this License, each Contributor hereby grants to You a perpetual,
   worldwide, non-exclusive, no-charge, royalty-free, irrevocable
   (except as stated in this section) patent license to make, have made,
   use, offer to sell, sell, import, and otherwise transfer the Work,
   where such license applies only to those patent claims licensable
   by such Contributor that are necessarily infringed by their
   Contribution(s) alone or by combination of their Contribution(s)
   with the Work to which such Contribution(s) was submitted. If You
   institute patent litigation against any entity (including a
   cross-claim or counterclaim in a lawsuit) alleging that the Work
   or a Contribution incorporated within the Work constitutes direct
   or contributory patent infringement, then any patent licenses
   granted to You under this License for that Work shall terminate
   as of the date such litigation is filed.

4. Redistribution. You may reproduce and distribute copies of the
   Work or Derivative Works thereof in any medium, with or without
   modifications, and in Source or Object form, provided that You
   meet the following conditions:

   (a) You must give any other recipients of the Work or
       Derivative Works a copy of this License; and

   (b) You must cause any modified files to carry prominent notices
       stating that You changed the files; and

   (c) You must retain, in the Source form of any Derivative Works
       that You distribute, all copyright, patent, trademark, and
       attribution notices from the Source form of the Work,
       excluding those notices that do not pertain to any part of
       the Derivative Works; and

   (d) If the Work includes a "NOTICE" text file as part of its
       distribution, then any Derivative Works that You distribute must
       include a readable copy of the attribution notices contained
       within such NOTICE file, excluding those notices that do not
       pertain to any part of the Derivative Works, in at least one
       of the following places: within a NOTICE text file distributed
       as part of the Derivative Works; within the Source form or
       documentation, if provided along with the Derivative Works; or,
       within a display generated by the Derivative Works, if and
       wherever such third-party notices normally appear. The contents
       of the NOTICE file are for informational purposes only and
       do not modify the License. You may add Your own attribution
       notices within Derivative Works that You distribute, alongside
       or as an addendum to the NOTICE text from the Work, provided
       that such additional attribution notices cannot be construed
       as modifying the License.

   You may add Your own copyright statement to Your modifications and
   may provide additional or different license terms and conditions
   for use, reproduction, or distribution of Your modifications, or
   for any such Derivative Works as a whole, provided Your use,
   reproduction, and distribution of the Work otherwise complies with
   the conditions stated in this License.

5. Submission of Contributions. Unless You explicitly state otherwise,
   any Contribution intentionally submitted for inclusion in the Work
   by You to the Licensor shall be under the terms and conditions of
   this License, without any additional terms or conditions.
   Notwithstanding the above, nothing herein shall supersede or modify
   the terms of any separate license agreement you may have executed
   with Licensor regarding such Contributions.

6. Trademarks. This License does not grant permission to use the trade
   names, trademarks, service marks, or product names of the Licensor,
   except as required for reasonable and customary use in describing the
   origin of the Work and reproducing the content of the NOTICE file.

7. Disclaimer of Warranty. Unless required by applicable law or
   agreed to in writing, Licensor provides the Work (and each
   Contributor provides its Contributions) on an "AS IS" BASIS,
   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or
   implied, including, without limitation, any warranties or conditions
   of TITLE, NON-INFRINGEMENT, MERCHANTABILITY, or FITNESS FOR A
   PARTICULAR PURPOSE. You are solely responsible for determining the
   appropriateness of using or redistributing the Work and assume any
   risks associated with Your exercise of permissions under this License.

8. Limitation of Liability. In no event and under no legal theory,
   whether in tort (including negligence), contract, or otherwise,
   unless required by applicable law (such as deliberate and grossly
   negligent acts) or agreed to in writing, shall any Contributor be
   liable to You for damages, including any direct, indirect, special,
   incidental, or consequential damages of any character arising as a
   result of this License or out of the use or inability to use the
   Work (including but not limited to damages for loss of goodwill,
   work stoppage, computer failure or malfunction, or any and all
   other commercial damages or losses), even if such Contributor
   has been advised of the possibility of such damages.

9. Accepting Warranty or Additional Liability. While redistributing
   the Work or Derivative Works thereof, You may choose to offer,
   and charge a fee for, acceptance of support, warranty, indemnity,
   or other liability obligations and/or rights consistent with this
   License. However, in accepting such obligations, You may act only
   on Your own behalf and on Your sole responsibility, not on behalf
   of any other Contributor, and only if You agree to indemnify,
   defend, and hold each Contributor harmless for any liability
   incurred by, or claims asserted against, such Contributor by reason
   of your accepting any such warranty or additional liability.

END OF TERMS AND CONDITIONS

APPENDIX: How to apply the Apache License to your work.

   To apply the Apache License to your work, attach the following
   boilerplate notice, with the fields enclosed by brackets "[]"
   replaced with your own identifying information. (Don't include
   the brackets!)  The text should be enclosed in the appropriate
   comment syntax for the file format. We also recommend that a
   file or class name and description of purpose be included on the
   same "printed page" as the copyright notice for easier
   identification within third-party archives.

Copyright [yyyy] [name of copyright owner]

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

	http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
~~~~
````

- `chrono` (MIT OR Apache-2.0): https://github.com/chronotope/chrono
  - Copyright (c) 2014, Kang Seonghoon.

## Apache License 2.0 (`Apache-2.0`)

````text
                                 Apache License
                           Version 2.0, January 2004
                        http://www.apache.org/licenses/

   TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION

   1. Definitions.

      "License" shall mean the terms and conditions for use, reproduction,
      and distribution as defined by Sections 1 through 9 of this document.

      "Licensor" shall mean the copyright owner or entity authorized by
      the copyright owner that is granting the License.

      "Legal Entity" shall mean the union of the acting entity and all
      other entities that control, are controlled by, or are under common
      control with that entity. For the purposes of this definition,
      "control" means (i) the power, direct or indirect, to cause the
      direction or management of such entity, whether by contract or
      otherwise, or (ii) ownership of fifty percent (50%) or more of the
      outstanding shares, or (iii) beneficial ownership of such entity.

      "You" (or "Your") shall mean an individual or Legal Entity
      exercising permissions granted by this License.

      "Source" form shall mean the preferred form for making modifications,
      including but not limited to software source code, documentation
      source, and configuration files.

      "Object" form shall mean any form resulting from mechanical
      transformation or translation of a Source form, including but
      not limited to compiled object code, generated documentation,
      and conversions to other media types.

      "Work" shall mean the work of authorship, whether in Source or
      Object form, made available under the License, as indicated by a
      copyright notice that is included in or attached to the work
      (an example is provided in the Appendix below).

      "Derivative Works" shall mean any work, whether in Source or Object
      form, that is based on (or derived from) the Work and for which the
      editorial revisions, annotations, elaborations, or other modifications
      represent, as a whole, an original work of authorship. For the purposes
      of this License, Derivative Works shall not include works that remain
      separable from, or merely link (or bind by name) to the interfaces of,
      the Work and Derivative Works thereof.

      "Contribution" shall mean any work of authorship, including
      the original version of the Work and any modifications or additions
      to that Work or Derivative Works thereof, that is intentionally
      submitted to Licensor for inclusion in the Work by the copyright owner
      or by an individual or Legal Entity authorized to submit on behalf of
      the copyright owner. For the purposes of this definition, "submitted"
      means any form of electronic, verbal, or written communication sent
      to the Licensor or its representatives, including but not limited to
      communication on electronic mailing lists, source code control systems,
      and issue tracking systems that are managed by, or on behalf of, the
      Licensor for the purpose of discussing and improving the Work, but
      excluding communication that is conspicuously marked or otherwise
      designated in writing by the copyright owner as "Not a Contribution."

      "Contributor" shall mean Licensor and any individual or Legal Entity
      on behalf of whom a Contribution has been received by Licensor and
      subsequently incorporated within the Work.

   2. Grant of Copyright License. Subject to the terms and conditions of
      this License, each Contributor hereby grants to You a perpetual,
      worldwide, non-exclusive, no-charge, royalty-free, irrevocable
      copyright license to reproduce, prepare Derivative Works of,
      publicly display, publicly perform, sublicense, and distribute the
      Work and such Derivative Works in Source or Object form.

   3. Grant of Patent License. Subject to the terms and conditions of
      this License, each Contributor hereby grants to You a perpetual,
      worldwide, non-exclusive, no-charge, royalty-free, irrevocable
      (except as stated in this section) patent license to make, have made,
      use, offer to sell, sell, import, and otherwise transfer the Work,
      where such license applies only to those patent claims licensable
      by such Contributor that are necessarily infringed by their
      Contribution(s) alone or by combination of their Contribution(s)
      with the Work to which such Contribution(s) was submitted. If You
      institute patent litigation against any entity (including a
      cross-claim or counterclaim in a lawsuit) alleging that the Work
      or a Contribution incorporated within the Work constitutes direct
      or contributory patent infringement, then any patent licenses
      granted to You under this License for that Work shall terminate
      as of the date such litigation is filed.

   4. Redistribution. You may reproduce and distribute copies of the
      Work or Derivative Works thereof in any medium, with or without
      modifications, and in Source or Object form, provided that You
      meet the following conditions:

      (a) You must give any other recipients of the Work or
          Derivative Works a copy of this License; and

      (b) You must cause any modified files to carry prominent notices
          stating that You changed the files; and

      (c) You must retain, in the Source form of any Derivative Works
          that You distribute, all copyright, patent, trademark, and
          attribution notices from the Source form of the Work,
          excluding those notices that do not pertain to any part of
          the Derivative Works; and

      (d) If the Work includes a "NOTICE" text file as part of its
          distribution, then any Derivative Works that You distribute must
          include a readable copy of the attribution notices contained
          within such NOTICE file, excluding those notices that do not
          pertain to any part of the Derivative Works, in at least one
          of the following places: within a NOTICE text file distributed
          as part of the Derivative Works; within the Source form or
          documentation, if provided along with the Derivative Works; or,
          within a display generated by the Derivative Works, if and
          wherever such third-party notices normally appear. The contents
          of the NOTICE file are for informational purposes only and
          do not modify the License. You may add Your own attribution
          notices within Derivative Works that You distribute, alongside
          or as an addendum to the NOTICE text from the Work, provided
          that such additional attribution notices cannot be construed
          as modifying the License.

      You may add Your own copyright statement to Your modifications and
      may provide additional or different license terms and conditions
      for use, reproduction, or distribution of Your modifications, or
      for any such Derivative Works as a whole, provided Your use,
      reproduction, and distribution of the Work otherwise complies with
      the conditions stated in this License.

   5. Submission of Contributions. Unless You explicitly state otherwise,
      any Contribution intentionally submitted for inclusion in the Work
      by You to the Licensor shall be under the terms and conditions of
      this License, without any additional terms or conditions.
      Notwithstanding the above, nothing herein shall supersede or modify
      the terms of any separate license agreement you may have executed
      with Licensor regarding such Contributions.

   6. Trademarks. This License does not grant permission to use the trade
      names, trademarks, service marks, or product names of the Licensor,
      except as required for reasonable and customary use in describing the
      origin of the Work and reproducing the content of the NOTICE file.

   7. Disclaimer of Warranty. Unless required by applicable law or
      agreed to in writing, Licensor provides the Work (and each
      Contributor provides its Contributions) on an "AS IS" BASIS,
      WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or
      implied, including, without limitation, any warranties or conditions
      of TITLE, NON-INFRINGEMENT, MERCHANTABILITY, or FITNESS FOR A
      PARTICULAR PURPOSE. You are solely responsible for determining the
      appropriateness of using or redistributing the Work and assume any
      risks associated with Your exercise of permissions under this License.

   8. Limitation of Liability. In no event and under no legal theory,
      whether in tort (including negligence), contract, or otherwise,
      unless required by applicable law (such as deliberate and grossly
      negligent acts) or agreed to in writing, shall any Contributor be
      liable to You for damages, including any direct, indirect, special,
      incidental, or consequential damages of any character arising as a
      result of this License or out of the use or inability to use the
      Work (including but not limited to damages for loss of goodwill,
      work stoppage, computer failure or malfunction, or any and all
      other commercial damages or losses), even if such Contributor
      has been advised of the possibility of such damages.

   9. Accepting Warranty or Additional Liability. While redistributing
      the Work or Derivative Works thereof, You may choose to offer,
      and charge a fee for, acceptance of support, warranty, indemnity,
      or other liability obligations and/or rights consistent with this
      License. However, in accepting such obligations, You may act only
      on Your own behalf and on Your sole responsibility, not on behalf
      of any other Contributor, and only if You agree to indemnify,
      defend, and hold each Contributor harmless for any liability
      incurred by, or claims asserted against, such Contributor by reason
      of your accepting any such warranty or additional liability.

   END OF TERMS AND CONDITIONS

   APPENDIX: How to apply the Apache License to your work.

      To apply the Apache License to your work, attach the following
      boilerplate notice, with the fields enclosed by brackets "[]"
      replaced with your own identifying information. (Don't include
      the brackets!)  The text should be enclosed in the appropriate
      comment syntax for the file format. We also recommend that a
      file or class name and description of purpose be included on the
      same "printed page" as the copyright notice for easier
      identification within third-party archives.

   Copyright [yyyy] [name of copyright owner]

   Licensed under the Apache License, Version 2.0 (the "License");
   you may not use this file except in compliance with the License.
   You may obtain a copy of the License at

       http://www.apache.org/licenses/LICENSE-2.0

   Unless required by applicable law or agreed to in writing, software
   distributed under the License is distributed on an "AS IS" BASIS,
   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
   See the License for the specific language governing permissions and
   limitations under the License.

--- LLVM Exceptions to the Apache 2.0 License ----

As an exception, if, as a result of your compiling your source code, portions
of this Software are embedded into an Object form of such source code, you
may redistribute such embedded portions in such Object form without complying
with the conditions of Sections 4(a), 4(b) and 4(d) of the License.

In addition, if you combine or link compiled forms of this Software with
software that is licensed under the GPLv2 ("Combined Software") and if a
court of competent jurisdiction determines that the patent provision (Section
3), the indemnity provision (Section 9) or other Section of the License
conflicts with the conditions of the GPLv2, you may retroactively and
prospectively choose to deem waived or otherwise exclude such Section(s) of
the License, but only in their entirety and only with respect to the Combined
Software.
````

- `linux-raw-sys` (Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT): https://github.com/sunfishcode/linux-raw-sys
- `rustix` (Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT): https://github.com/bytecodealliance/rustix
- `target-lexicon` (Apache-2.0 WITH LLVM-exception): https://github.com/bytecodealliance/target-lexicon

## Apache License 2.0 (`Apache-2.0`)

````text
                                 Apache License
                           Version 2.0, January 2004
                        http://www.apache.org/licenses/

   TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION

   1. Definitions.

      "License" shall mean the terms and conditions for use, reproduction,
      and distribution as defined by Sections 1 through 9 of this document.

      "Licensor" shall mean the copyright owner or entity authorized by
      the copyright owner that is granting the License.

      "Legal Entity" shall mean the union of the acting entity and all
      other entities that control, are controlled by, or are under common
      control with that entity. For the purposes of this definition,
      "control" means (i) the power, direct or indirect, to cause the
      direction or management of such entity, whether by contract or
      otherwise, or (ii) ownership of fifty percent (50%) or more of the
      outstanding shares, or (iii) beneficial ownership of such entity.

      "You" (or "Your") shall mean an individual or Legal Entity
      exercising permissions granted by this License.

      "Source" form shall mean the preferred form for making modifications,
      including but not limited to software source code, documentation
      source, and configuration files.

      "Object" form shall mean any form resulting from mechanical
      transformation or translation of a Source form, including but
      not limited to compiled object code, generated documentation,
      and conversions to other media types.

      "Work" shall mean the work of authorship, whether in Source or
      Object form, made available under the License, as indicated by a
      copyright notice that is included in or attached to the work
      (an example is provided in the Appendix below).

      "Derivative Works" shall mean any work, whether in Source or Object
      form, that is based on (or derived from) the Work and for which the
      editorial revisions, annotations, elaborations, or other modifications
      represent, as a whole, an original work of authorship. For the purposes
      of this License, Derivative Works shall not include works that remain
      separable from, or merely link (or bind by name) to the interfaces of,
      the Work and Derivative Works thereof.

      "Contribution" shall mean any work of authorship, including
      the original version of the Work and any modifications or additions
      to that Work or Derivative Works thereof, that is intentionally
      submitted to Licensor for inclusion in the Work by the copyright owner
      or by an individual or Legal Entity authorized to submit on behalf of
      the copyright owner. For the purposes of this definition, "submitted"
      means any form of electronic, verbal, or written communication sent
      to the Licensor or its representatives, including but not limited to
      communication on electronic mailing lists, source code control systems,
      and issue tracking systems that are managed by, or on behalf of, the
      Licensor for the purpose of discussing and improving the Work, but
      excluding communication that is conspicuously marked or otherwise
      designated in writing by the copyright owner as "Not a Contribution."

      "Contributor" shall mean Licensor and any individual or Legal Entity
      on behalf of whom a Contribution has been received by Licensor and
      subsequently incorporated within the Work.

   2. Grant of Copyright License. Subject to the terms and conditions of
      this License, each Contributor hereby grants to You a perpetual,
      worldwide, non-exclusive, no-charge, royalty-free, irrevocable
      copyright license to reproduce, prepare Derivative Works of,
      publicly display, publicly perform, sublicense, and distribute the
      Work and such Derivative Works in Source or Object form.

   3. Grant of Patent License. Subject to the terms and conditions of
      this License, each Contributor hereby grants to You a perpetual,
      worldwide, non-exclusive, no-charge, royalty-free, irrevocable
      (except as stated in this section) patent license to make, have made,
      use, offer to sell, sell, import, and otherwise transfer the Work,
      where such license applies only to those patent claims licensable
      by such Contributor that are necessarily infringed by their
      Contribution(s) alone or by combination of their Contribution(s)
      with the Work to which such Contribution(s) was submitted. If You
      institute patent litigation against any entity (including a
      cross-claim or counterclaim in a lawsuit) alleging that the Work
      or a Contribution incorporated within the Work constitutes direct
      or contributory patent infringement, then any patent licenses
      granted to You under this License for that Work shall terminate
      as of the date such litigation is filed.

   4. Redistribution. You may reproduce and distribute copies of the
      Work or Derivative Works thereof in any medium, with or without
      modifications, and in Source or Object form, provided that You
      meet the following conditions:

      (a) You must give any other recipients of the Work or
          Derivative Works a copy of this License; and

      (b) You must cause any modified files to carry prominent notices
          stating that You changed the files; and

      (c) You must retain, in the Source form of any Derivative Works
          that You distribute, all copyright, patent, trademark, and
          attribution notices from the Source form of the Work,
          excluding those notices that do not pertain to any part of
          the Derivative Works; and

      (d) If the Work includes a "NOTICE" text file as part of its
          distribution, then any Derivative Works that You distribute must
          include a readable copy of the attribution notices contained
          within such NOTICE file, excluding those notices that do not
          pertain to any part of the Derivative Works, in at least one
          of the following places: within a NOTICE text file distributed
          as part of the Derivative Works; within the Source form or
          documentation, if provided along with the Derivative Works; or,
          within a display generated by the Derivative Works, if and
          wherever such third-party notices normally appear. The contents
          of the NOTICE file are for informational purposes only and
          do not modify the License. You may add Your own attribution
          notices within Derivative Works that You distribute, alongside
          or as an addendum to the NOTICE text from the Work, provided
          that such additional attribution notices cannot be construed
          as modifying the License.

      You may add Your own copyright statement to Your modifications and
      may provide additional or different license terms and conditions
      for use, reproduction, or distribution of Your modifications, or
      for any such Derivative Works as a whole, provided Your use,
      reproduction, and distribution of the Work otherwise complies with
      the conditions stated in this License.

   5. Submission of Contributions. Unless You explicitly state otherwise,
      any Contribution intentionally submitted for inclusion in the Work
      by You to the Licensor shall be under the terms and conditions of
      this License, without any additional terms or conditions.
      Notwithstanding the above, nothing herein shall supersede or modify
      the terms of any separate license agreement you may have executed
      with Licensor regarding such Contributions.

   6. Trademarks. This License does not grant permission to use the trade
      names, trademarks, service marks, or product names of the Licensor,
      except as required for reasonable and customary use in describing the
      origin of the Work and reproducing the content of the NOTICE file.

   7. Disclaimer of Warranty. Unless required by applicable law or
      agreed to in writing, Licensor provides the Work (and each
      Contributor provides its Contributions) on an "AS IS" BASIS,
      WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or
      implied, including, without limitation, any warranties or conditions
      of TITLE, NON-INFRINGEMENT, MERCHANTABILITY, or FITNESS FOR A
      PARTICULAR PURPOSE. You are solely responsible for determining the
      appropriateness of using or redistributing the Work and assume any
      risks associated with Your exercise of permissions under this License.

   8. Limitation of Liability. In no event and under no legal theory,
      whether in tort (including negligence), contract, or otherwise,
      unless required by applicable law (such as deliberate and grossly
      negligent acts) or agreed to in writing, shall any Contributor be
      liable to You for damages, including any direct, indirect, special,
      incidental, or consequential damages of any character arising as a
      result of this License or out of the use or inability to use the
      Work (including but not limited to damages for loss of goodwill,
      work stoppage, computer failure or malfunction, or any and all
      other commercial damages or losses), even if such Contributor
      has been advised of the possibility of such damages.

   9. Accepting Warranty or Additional Liability. While redistributing
      the Work or Derivative Works thereof, You may choose to offer,
      and charge a fee for, acceptance of support, warranty, indemnity,
      or other liability obligations and/or rights consistent with this
      License. However, in accepting such obligations, You may act only
      on Your own behalf and on Your sole responsibility, not on behalf
      of any other Contributor, and only if You agree to indemnify,
      defend, and hold each Contributor harmless for any liability
      incurred by, or claims asserted against, such Contributor by reason
      of your accepting any such warranty or additional liability.

   END OF TERMS AND CONDITIONS

   APPENDIX: How to apply the Apache License to your work.

      To apply the Apache License to your work, attach the following
      boilerplate notice, with the fields enclosed by brackets "[]"
      replaced with your own identifying information. (Don't include
      the brackets!)  The text should be enclosed in the appropriate
      comment syntax for the file format. We also recommend that a
      file or class name and description of purpose be included on the
      same "printed page" as the copyright notice for easier
      identification within third-party archives.

   Copyright [yyyy] [name of copyright owner]

   Licensed under the Apache License, Version 2.0 (the "License");
   you may not use this file except in compliance with the License.
   You may obtain a copy of the License at

       http://www.apache.org/licenses/LICENSE-2.0

   Unless required by applicable law or agreed to in writing, software
   distributed under the License is distributed on an "AS IS" BASIS,
   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
   See the License for the specific language governing permissions and
   limitations under the License.


Licenses for support code
-------------------------

Parts of the TLS test suite are under the Go license. This code is not included
in BoringSSL (i.e. libcrypto and libssl) when compiled, however, so
distributing code linked against BoringSSL does not trigger this license:

Copyright (c) 2009 The Go Authors. All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

   * Redistributions of source code must retain the above copyright
notice, this list of conditions and the following disclaimer.
   * Redistributions in binary form must reproduce the above
copyright notice, this list of conditions and the following disclaimer
in the documentation and/or other materials provided with the
distribution.
   * Neither the name of Google Inc. nor the names of its
contributors may be used to endorse or promote products derived from
this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.


BoringSSL uses the Chromium test infrastructure to run a continuous build,
trybots etc. The scripts which manage this, and the script for generating build
metadata, are under the Chromium license. Distributing code linked against
BoringSSL does not trigger this license.

Copyright 2015 The Chromium Authors. All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

   * Redistributions of source code must retain the above copyright
notice, this list of conditions and the following disclaimer.
   * Redistributions in binary form must reproduce the above
copyright notice, this list of conditions and the following disclaimer
in the documentation and/or other materials provided with the
distribution.
   * Neither the name of Google Inc. nor the names of its
contributors may be used to endorse or promote products derived from
this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
````

- `ring` (Apache-2.0 AND ISC): https://github.com/briansmith/ring

## Apache License 2.0 (`Apache-2.0`)

````text
unrar-rs license

unrar-rs is licensed under the Apache License, Version 2.0, with one further
condition on its RAR engine.

The RAR engine was developed using the source code of the unRAR program. All
copyrights to the original unRAR code are owned by Alexander Roshal, and its
license restriction continues to govern that code:

UnRAR source code may be used in any software to handle
RAR archives without limitations free of charge, but cannot be
used to develop RAR (WinRAR) compatible archiver and to
re-create RAR compression algorithm, which is proprietary.
Distribution of modified UnRAR source code in separate form
or as a part of other software is permitted, provided that
full text of this paragraph, starting from "UnRAR source code"
words, is included in license, or in documentation if license
is not available, and in source code comments of resulting package.

A binary distribution must include this file.

The full Apache License, Version 2.0 text follows.

                                 Apache License
                           Version 2.0, January 2004
                        http://www.apache.org/licenses/

   TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION

   1. Definitions.

      "License" shall mean the terms and conditions for use, reproduction,
      and distribution as defined by Sections 1 through 9 of this document.

      "Licensor" shall mean the copyright owner or entity authorized by
      the copyright owner that is granting the License.

      "Legal Entity" shall mean the union of the acting entity and all
      other entities that control, are controlled by, or are under common
      control with that entity. For the purposes of this definition,
      "control" means (i) the power, direct or indirect, to cause the
      direction or management of such entity, whether by contract or
      otherwise, or (ii) ownership of fifty percent (50%) or more of the
      outstanding shares, or (iii) beneficial ownership of such entity.

      "You" (or "Your") shall mean an individual or Legal Entity
      exercising permissions granted by this License.

      "Source" form shall mean the preferred form for making modifications,
      including but not limited to software source code, documentation
      source, and configuration files.

      "Object" form shall mean any form resulting from mechanical
      transformation or translation of a Source form, including but
      not limited to compiled object code, generated documentation,
      and conversions to other media types.

      "Work" shall mean the work of authorship, whether in Source or
      Object form, made available under the License, as indicated by a
      copyright notice that is included in or attached to the work
      (an example is provided in the Appendix below).

      "Derivative Works" shall mean any work, whether in Source or Object
      form, that is based on (or derived from) the Work and for which the
      editorial revisions, annotations, elaborations, or other modifications
      represent, as a whole, an original work of authorship. For the purposes
      of this License, Derivative Works shall not include works that remain
      separable from, or merely link (or bind by name) to the interfaces of,
      the Work and Derivative Works thereof.

      "Contribution" shall mean any work of authorship, including
      the original version of the Work and any modifications or additions
      to that Work or Derivative Works thereof, that is intentionally
      submitted to Licensor for inclusion in the Work by the copyright owner
      or by an individual or Legal Entity authorized to submit on behalf of
      the copyright owner. For the purposes of this definition, "submitted"
      means any form of electronic, verbal, or written communication sent
      to the Licensor or its representatives, including but not limited to
      communication on electronic mailing lists, source code control systems,
      and issue tracking systems that are managed by, or on behalf of, the
      Licensor for the purpose of discussing and improving the Work, but
      excluding communication that is conspicuously marked or otherwise
      designated in writing by the copyright owner as "Not a Contribution."

      "Contributor" shall mean Licensor and any individual or Legal Entity
      on behalf of whom a Contribution has been received by Licensor and
      subsequently incorporated within the Work.

   2. Grant of Copyright License. Subject to the terms and conditions of
      this License, each Contributor hereby grants to You a perpetual,
      worldwide, non-exclusive, no-charge, royalty-free, irrevocable
      copyright license to reproduce, prepare Derivative Works of,
      publicly display, publicly perform, sublicense, and distribute the
      Work and such Derivative Works in Source or Object form.

   3. Grant of Patent License. Subject to the terms and conditions of
      this License, each Contributor hereby grants to You a perpetual,
      worldwide, non-exclusive, no-charge, royalty-free, irrevocable
      (except as stated in this section) patent license to make, have made,
      use, offer to sell, sell, import, and otherwise transfer the Work,
      where such license applies only to those patent claims licensable
      by such Contributor that are necessarily infringed by their
      Contribution(s) alone or by combination of their Contribution(s)
      with the Work to which such Contribution(s) was submitted. If You
      institute patent litigation against any entity (including a
      cross-claim or counterclaim in a lawsuit) alleging that the Work
      or a Contribution incorporated within the Work constitutes direct
      or contributory patent infringement, then any patent licenses
      granted to You under this License for that Work shall terminate
      as of the date such litigation is filed.

   4. Redistribution. You may reproduce and distribute copies of the
      Work or Derivative Works thereof in any medium, with or without
      modifications, and in Source or Object form, provided that You
      meet the following conditions:

      (a) You must give any other recipients of the Work or
          Derivative Works a copy of this License; and

      (b) You must cause any modified files to carry prominent notices
          stating that You changed the files; and

      (c) You must retain, in the Source form of any Derivative Works
          that You distribute, all copyright, patent, trademark, and
          attribution notices from the Source form of the Work,
          excluding those notices that do not pertain to any part of
          the Derivative Works; and

      (d) If the Work includes a "NOTICE" text file as part of its
          distribution, then any Derivative Works that You distribute must
          include a readable copy of the attribution notices contained
          within such NOTICE file, excluding those notices that do not
          pertain to any part of the Derivative Works, in at least one
          of the following places: within a NOTICE text file distributed
          as part of the Derivative Works; within the Source form or
          documentation, if provided along with the Derivative Works; or,
          within a display generated by the Derivative Works, if and
          wherever such third-party notices normally appear. The contents
          of the NOTICE file are for informational purposes only and
          do not modify the License. You may add Your own attribution
          notices within Derivative Works that You distribute, alongside
          or as an addendum to the NOTICE text from the Work, provided
          that such additional attribution notices cannot be construed
          as modifying the License.

      You may add Your own copyright statement to Your modifications and
      may provide additional or different license terms and conditions
      for use, reproduction, or distribution of Your modifications, or
      for any such Derivative Works as a whole, provided Your use,
      reproduction, and distribution of the Work otherwise complies with
      the conditions stated in this License.

   5. Submission of Contributions. Unless You explicitly state otherwise,
      any Contribution intentionally submitted for inclusion in the Work
      by You to the Licensor shall be under the terms and conditions of
      this License, without any additional terms or conditions.
      Notwithstanding the above, nothing herein shall supersede or modify
      the terms of any separate license agreement you may have executed
      with Licensor regarding such Contributions.

   6. Trademarks. This License does not grant permission to use the trade
      names, trademarks, service marks, or product names of the Licensor,
      except as required for reasonable and customary use in describing the
      origin of the Work and reproducing the content of the NOTICE file.

   7. Disclaimer of Warranty. Unless required by applicable law or
      agreed to in writing, Licensor provides the Work (and each
      Contributor provides its Contributions) on an "AS IS" BASIS,
      WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or
      implied, including, without limitation, any warranties or conditions
      of TITLE, NON-INFRINGEMENT, MERCHANTABILITY, or FITNESS FOR A
      PARTICULAR PURPOSE. You are solely responsible for determining the
      appropriateness of using or redistributing the Work and assume any
      risks associated with Your exercise of permissions under this License.

   8. Limitation of Liability. In no event and under no legal theory,
      whether in tort (including negligence), contract, or otherwise,
      unless required by applicable law (such as deliberate and grossly
      negligent acts) or agreed to in writing, shall any Contributor be
      liable to You for damages, including any direct, indirect, special,
      incidental, or consequential damages of any character arising as a
      result of this License or out of the use or inability to use the
      Work (including but not limited to damages for loss of goodwill,
      work stoppage, computer failure or malfunction, or any and all
      other commercial damages or losses), even if such Contributor
      has been advised of the possibility of such damages.

   9. Accepting Warranty or Additional Liability. While redistributing
      the Work or Derivative Works thereof, You may choose to offer,
      and charge a fee for, acceptance of support, warranty, indemnity,
      or other liability obligations and/or rights consistent with this
      License. However, in accepting such obligations, You may act only
      on Your own behalf and on Your sole responsibility, not on behalf
      of any other Contributor, and only if You agree to indemnify,
      defend, and hold each Contributor harmless for any liability
      incurred by, or claims asserted against, such Contributor by reason
      of your accepting any such warranty or additional liability.

   END OF TERMS AND CONDITIONS

   APPENDIX: How to apply the Apache License to your work.

      To apply the Apache License to your work, attach the following
      boilerplate notice, with the fields enclosed by brackets "[]"
      replaced with your own identifying information. (Don't include
      the brackets!)  The text should be enclosed in the appropriate
      comment syntax for the file format. We also recommend that a
      file or class name and description of purpose be included on the
      same "printed page" as the copyright notice for easier
      identification within third-party archives.

   Copyright [yyyy] [name of copyright owner]

   Licensed under the Apache License, Version 2.0 (the "License");
   you may not use this file except in compliance with the License.
   You may obtain a copy of the License at

       http://www.apache.org/licenses/LICENSE-2.0

   Unless required by applicable law or agreed to in writing, software
   distributed under the License is distributed on an "AS IS" BASIS,
   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
   See the License for the specific language governing permissions and
   limitations under the License.
````

- `unrar-rs`: https://github.com/scryer-media/rarpar

## BSD 2-Clause "Simplified" License (`BSD-2-Clause`)

````text
All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

1. Redistributions of source code must retain the above copyright
   notice, this list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright
   notice, this list of conditions and the following disclaimer in the
   documentation and/or other materials provided with the
   distribution.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
````

- `arrayref` (BSD-2-Clause): https://github.com/droundy/arrayref
  - Copyright (c) 2015 David Roundy <roundyd@physics.oregonstate.edu>

## BSD 3-Clause "New" or "Revised" License (`BSD-3-Clause`)

````text
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions are met:
//
// 1. Redistributions of source code must retain the above copyright notice, this
//    list of conditions and the following disclaimer.
//
// 2. Redistributions in binary form must reproduce the above copyright notice,
//    this list of conditions and the following disclaimer in the documentation
//    and/or other materials provided with the distribution.
//
// 3. Neither the name of the copyright holder nor the names of its
//    contributors may be used to endorse or promote products derived from
//    this software without specific prior written permission.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
// AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
// IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
// DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
// FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
// DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
// SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
// CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
// OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
// OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

/// The PUA code points special-cased in the GB18030 encoder.
pub(crate) static GB18030_2022_OVERRIDE_PUA: [u16; 18] = [
    0xE78D, 0xE78E, 0xE78F, 0xE790, 0xE791, 0xE792, 0xE793, 0xE794, 0xE795, 0xE796, 0xE81E, 0xE826,
    0xE82B, 0xE82C, 0xE832, 0xE843, 0xE854, 0xE864,
];

/// The bytes corresponding to the PUA code points special-cased in the GB18030 encoder.
pub(crate) static GB18030_2022_OVERRIDE_BYTES: [[u8; 2]; 18] = [
    [0xA6, 0xD9],
    [0xA6, 0xDA],
    [0xA6, 0xDB],
    [0xA6, 0xDC],
    [0xA6, 0xDD],
    [0xA6, 0xDE],
    [0xA6, 0xDF],
    [0xA6, 0xEC],
    [0xA6, 0xED],
    [0xA6, 0xF3],
    [0xFE, 0x59],
    [0xFE, 0x61],
    [0xFE, 0x66],
    [0xFE, 0x67],
    [0xFE, 0x6D],
    [0xFE, 0x7E],
    [0xFE, 0x90],
    [0xFE, 0xA0],
];
````

- `encoding_rs` ((Apache-2.0 OR MIT) AND BSD-3-Clause): https://github.com/hsivonen/encoding_rs
  - // Copyright © WHATWG (Apple, Google, Mozilla, Microsoft).

## BSD 3-Clause "New" or "Revised" License (`BSD-3-Clause`)

````text
Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

1. Redistributions of source code must retain the above copyright
notice, this list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright
notice, this list of conditions and the following disclaimer in the
documentation and/or other materials provided with the distribution.

3. Neither the name of the copyright holder nor the names of its
contributors may be used to endorse or promote products derived from
this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS
IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A
PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED
TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
````

- `encoding_rs` ((Apache-2.0 OR MIT) AND BSD-3-Clause): https://github.com/hsivonen/encoding_rs
  - Copyright © WHATWG (Apple, Google, Mozilla, Microsoft).
- `subtle` (BSD-3-Clause): https://github.com/dalek-cryptography/subtle
  - Copyright (c) 2016-2017 Isis Agora Lovecruft, Henry de Valence. All rights reserved.
  - Copyright (c) 2016-2024 Isis Agora Lovecruft. All rights reserved.

## BSD 3-Clause "New" or "Revised" License (`BSD-3-Clause`)

````text
Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

  * Redistributions of source code must retain the above copyright
    notice, this list of conditions and the following disclaimer.

  * Redistributions in binary form must reproduce the above copyright
    notice, this list of conditions and the following disclaimer in
    the documentation and/or other materials provided with the
    distribution.

  * Neither the name of the copyright holder nor the names of its
    contributors may be used to endorse or promote products derived
    from this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
````

- `tiny-skia` (BSD-3-Clause): https://github.com/RazrFalcon/tiny-skia
  - Copyright (c) 2011 Google Inc. All rights reserved.
  - Copyright (c) 2020 Yevhenii Reizner All rights reserved.
- `tiny-skia-path` (BSD-3-Clause): https://github.com/RazrFalcon/tiny-skia/tree/master/path
  - Copyright (c) 2011 Google Inc. All rights reserved.
  - Copyright (c) 2020 Yevhenii Reizner All rights reserved.

## Creative Commons Zero v1.0 Universal (`CC0-1.0`)

````text
CC0 1.0 Universal

    CREATIVE COMMONS CORPORATION IS NOT A LAW FIRM AND DOES NOT PROVIDE
    LEGAL SERVICES. DISTRIBUTION OF THIS DOCUMENT DOES NOT CREATE AN
    ATTORNEY-CLIENT RELATIONSHIP. CREATIVE COMMONS PROVIDES THIS
    INFORMATION ON AN "AS-IS" BASIS. CREATIVE COMMONS MAKES NO WARRANTIES
    REGARDING THE USE OF THIS DOCUMENT OR THE INFORMATION OR WORKS
    PROVIDED HEREUNDER, AND DISCLAIMS LIABILITY FOR DAMAGES RESULTING FROM
    THE USE OF THIS DOCUMENT OR THE INFORMATION OR WORKS PROVIDED
    HEREUNDER.

Statement of Purpose

The laws of most jurisdictions throughout the world automatically confer
exclusive Copyright and Related Rights (defined below) upon the creator
and subsequent owner(s) (each and all, an "owner") of an original work of
authorship and/or a database (each, a "Work").

Certain owners wish to permanently relinquish those rights to a Work for
the purpose of contributing to a commons of creative, cultural and
scientific works ("Commons") that the public can reliably and without fear
of later claims of infringement build upon, modify, incorporate in other
works, reuse and redistribute as freely as possible in any form whatsoever
and for any purposes, including without limitation commercial purposes.
These owners may contribute to the Commons to promote the ideal of a free
culture and the further production of creative, cultural and scientific
works, or to gain reputation or greater distribution for their Work in
part through the use and efforts of others.

For these and/or other purposes and motivations, and without any
expectation of additional consideration or compensation, the person
associating CC0 with a Work (the "Affirmer"), to the extent that he or she
is an owner of Copyright and Related Rights in the Work, voluntarily
elects to apply CC0 to the Work and publicly distribute the Work under its
terms, with knowledge of his or her Copyright and Related Rights in the
Work and the meaning and intended legal effect of CC0 on those rights.

1. Copyright and Related Rights. A Work made available under CC0 may be
   protected by copyright and related or neighboring rights ("Copyright and
   Related Rights"). Copyright and Related Rights include, but are not
   limited to, the following:

i. the right to reproduce, adapt, distribute, perform, display,
communicate, and translate a Work;
ii. moral rights retained by the original author(s) and/or performer(s);
iii. publicity and privacy rights pertaining to a person's image or
likeness depicted in a Work;
iv. rights protecting against unfair competition in regards to a Work,
subject to the limitations in paragraph 4(a), below;
v. rights protecting the extraction, dissemination, use and reuse of data
in a Work;
vi. database rights (such as those arising under Directive 96/9/EC of the
European Parliament and of the Council of 11 March 1996 on the legal
protection of databases, and under any national implementation
thereof, including any amended or successor version of such
directive); and
vii. other similar, equivalent or corresponding rights throughout the
world based on applicable law or treaty, and any national
implementations thereof.

2. Waiver. To the greatest extent permitted by, but not in contravention
   of, applicable law, Affirmer hereby overtly, fully, permanently,
   irrevocably and unconditionally waives, abandons, and surrenders all of
   Affirmer's Copyright and Related Rights and associated claims and causes
   of action, whether now known or unknown (including existing as well as
   future claims and causes of action), in the Work (i) in all territories
   worldwide, (ii) for the maximum duration provided by applicable law or
   treaty (including future time extensions), (iii) in any current or future
   medium and for any number of copies, and (iv) for any purpose whatsoever,
   including without limitation commercial, advertising or promotional
   purposes (the "Waiver"). Affirmer makes the Waiver for the benefit of each
   member of the public at large and to the detriment of Affirmer's heirs and
   successors, fully intending that such Waiver shall not be subject to
   revocation, rescission, cancellation, termination, or any other legal or
   equitable action to disrupt the quiet enjoyment of the Work by the public
   as contemplated by Affirmer's express Statement of Purpose.

3. Public License Fallback. Should any part of the Waiver for any reason
   be judged legally invalid or ineffective under applicable law, then the
   Waiver shall be preserved to the maximum extent permitted taking into
   account Affirmer's express Statement of Purpose. In addition, to the
   extent the Waiver is so judged Affirmer hereby grants to each affected
   person a royalty-free, non transferable, non sublicensable, non exclusive,
   irrevocable and unconditional license to exercise Affirmer's Copyright and
   Related Rights in the Work (i) in all territories worldwide, (ii) for the
   maximum duration provided by applicable law or treaty (including future
   time extensions), (iii) in any current or future medium and for any number
   of copies, and (iv) for any purpose whatsoever, including without
   limitation commercial, advertising or promotional purposes (the
   "License"). The License shall be deemed effective as of the date CC0 was
   applied by Affirmer to the Work. Should any part of the License for any
   reason be judged legally invalid or ineffective under applicable law, such
   partial invalidity or ineffectiveness shall not invalidate the remainder
   of the License, and in such case Affirmer hereby affirms that he or she
   will not (i) exercise any of his or her remaining Copyright and Related
   Rights in the Work or (ii) assert any associated claims and causes of
   action with respect to the Work, in either case contrary to Affirmer's
   express Statement of Purpose.

4. Limitations and Disclaimers.

a. No trademark or patent rights held by Affirmer are waived, abandoned,
surrendered, licensed or otherwise affected by this document.
b. Affirmer offers the Work as-is and makes no representations or
warranties of any kind concerning the Work, express, implied,
statutory or otherwise, including without limitation warranties of
title, merchantability, fitness for a particular purpose, non
infringement, or the absence of latent or other defects, accuracy, or
the present or absence of errors, whether or not discoverable, all to
the greatest extent permissible under applicable law.
c. Affirmer disclaims responsibility for clearing rights of other persons
that may apply to the Work or any use thereof, including without
limitation any person's Copyright and Related Rights in the Work.
Further, Affirmer disclaims responsibility for obtaining any necessary
consents, permissions or other rights required for any use of the
Work.
d. Affirmer understands and acknowledges that Creative Commons is not a
party to this document and has no duty or obligation with respect to
this CC0 or use of the Work.
````

- `ppmd-rust` (CC0-1.0 OR MIT-0): https://github.com/hasenbanck/ppmd-rust

## Community Data License Agreement Permissive 2.0 (`CDLA-Permissive-2.0`)

````text
# Community Data License Agreement - Permissive - Version 2.0

This is the Community Data License Agreement - Permissive, Version
2.0 (the "agreement"). Data Provider(s) and Data Recipient(s) agree
as follows:

## 1. Provision of the Data

1.1. A Data Recipient may use, modify, and share the Data made
available by Data Provider(s) under this agreement if that Data
Recipient follows the terms of this agreement.

1.2. This agreement does not impose any restriction on a Data
Recipient's use, modification, or sharing of any portions of the
Data that are in the public domain or that may be used, modified,
or shared under any other legal exception or limitation.

## 2. Conditions for Sharing Data

2.1. A Data Recipient may share Data, with or without modifications, so
long as the Data Recipient makes available the text of this agreement
with the shared Data.

## 3. No Restrictions on Results

3.1. This agreement does not impose any restriction or obligations
with respect to the use, modification, or sharing of Results.

## 4. No Warranty; Limitation of Liability

4.1. All Data Recipients receive the Data subject to the following
terms:

THE DATA IS PROVIDED ON AN "AS IS" BASIS, WITHOUT REPRESENTATIONS,
WARRANTIES OR CONDITIONS OF ANY KIND, EITHER EXPRESS OR IMPLIED
INCLUDING, WITHOUT LIMITATION, ANY WARRANTIES OR CONDITIONS OF TITLE,
NON-INFRINGEMENT, MERCHANTABILITY OR FITNESS FOR A PARTICULAR PURPOSE.

NO DATA PROVIDER SHALL HAVE ANY LIABILITY FOR ANY DIRECT, INDIRECT,
INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING
WITHOUT LIMITATION LOST PROFITS), HOWEVER CAUSED AND ON ANY THEORY OF
LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE DATA OR RESULTS,
EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGES.

## 5. Definitions

5.1. "Data" means the material received by a Data Recipient under
this agreement.

5.2. "Data Provider" means any person who is the source of Data
provided under this agreement and in reliance on a Data Recipient's
agreement to its terms.

5.3. "Data Recipient" means any person who receives Data directly
or indirectly from a Data Provider and agrees to the terms of this
agreement.

5.4. "Results" means any outcome obtained by computational analysis
of Data, including for example machine learning models and models'
insights.
````

- `webpki-root-certs` (CDLA-Permissive-2.0): https://github.com/rustls/webpki-roots
- `webpki-roots` (CDLA-Permissive-2.0): https://github.com/rustls/webpki-roots

## ISC License (`ISC`)

````text
Permission to use, copy, modify, and/or distribute this software for any purpose
with or without fee is hereby granted, provided that the above copyright notice
and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH
REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND
FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT,
INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS
OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER
TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF
THIS SOFTWARE.
````

- `inotify` (ISC): https://github.com/hannobraun/inotify-rs
  - Copyright (c) Hanno Braun and contributors
- `inotify-sys` (ISC): https://github.com/hannobraun/inotify-sys
  - Copyright (c) Hanno Braun and contributors
- `libloading` (ISC): https://github.com/nagisa/rust_libloading/
  - Copyright © 2015, Simonas Kazlauskas

## ISC License (`ISC`)

````text
Copyright 2015-2025 Brian Smith.

Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY
SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION
OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN
CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
````

- `ring` (Apache-2.0 AND ISC): https://github.com/briansmith/ring

## ISC License (`ISC`)

````text
Except as otherwise noted, this project is licensed under the following
(ISC-style) terms:

Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHORS DISCLAIM ALL WARRANTIES
WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR
ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

The files under third-party/chromium are licensed as described in
third-party/chromium/LICENSE.
````

- `rustls-webpki` (ISC): https://github.com/rustls/webpki
  - Copyright 2015 Brian Smith.

## ISC License (`ISC`)

````text
//
// Permission to use, copy, modify, and/or distribute this software for any
// purpose with or without fee is hereby granted, provided that the above
// copyright notice and this permission notice appear in all copies.
//
// THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHORS DISCLAIM ALL WARRANTIES
// WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
// MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR
// ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
// WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
// ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
// OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
````

- `untrusted` (ISC): https://github.com/briansmith/untrusted
  - // Copyright 2015-2016 Brian Smith.

## MIT License (`MIT`)

````text
MIT License

Copyright (c) <year> <copyright holders>

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and
associated documentation files (the "Software"), to deal in the Software without restriction, including
without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the
following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial
portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT
LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO
EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER
IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE
USE OR OTHER DEALINGS IN THE SOFTWARE.
````

- `aho-corasick` (Unlicense OR MIT): https://github.com/BurntSushi/aho-corasick
  - Copyright (c) 2015 Andrew Gallant
- `alsa-sys` (MIT): https://github.com/diwic/alsa-sys
  - Copyright (c) 2018 diwic
- `android-properties` (MIT): https://github.com/miklelappo/android-properties
  - Copyright (c) 2020 Mikhail Lappo
- `atk` (MIT): https://github.com/gtk-rs/gtk3-rs
- `atk-sys` (MIT): https://github.com/gtk-rs/gtk3-rs
- `blake2s_simd` (MIT): https://github.com/oconnor663/blake2_simd; ships no file with this licence's text; authors: Jack O'Connor
- `block2` (MIT): https://github.com/madsmtm/objc2; ships no file with this licence's text; authors: Mads Marquart <mads@marquart.dk>, Steven Sheldon
- `byteorder-lite` (Unlicense OR MIT): https://github.com/image-rs/byteorder-lite
  - Copyright (c) 2015 Andrew Gallant
- `bytes` (MIT): https://github.com/tokio-rs/bytes
  - Copyright (c) 2018 Carl Lerche
- `cairo-rs` (MIT): https://github.com/gtk-rs/gtk-rs-core
- `cairo-sys-rs` (MIT): https://github.com/gtk-rs/gtk-rs-core
- `calloop` (MIT): https://github.com/Smithay/calloop
  - Copyright (c) 2018 Victor Berger
- `calloop-wayland-source` (MIT): https://github.com/smithay/calloop-wayland-source
  - Copyright (c) 2023 Kirill Chibisov
- `cfg_aliases` (MIT): https://github.com/katharostech/cfg_aliases
  - Copyright (c) 2020 Katharos Technology
- `color_quant` (MIT): https://github.com/image-rs/color_quant.git
  - Copyright (c) 2016 PistonDevelopers
- `combine` (MIT): https://github.com/Marwes/combine
  - Copyright (c) 2015 Markus Westerlind
- `derive_more` (MIT): https://github.com/JelteF/derive_more
  - Copyright (c) 2016 Jelte Fennema
- `derive_more-impl` (MIT): https://github.com/JelteF/derive_more
  - Copyright (c) 2016 Jelte Fennema
- `dispatch` (MIT): http://github.com/SSheldon/rust-dispatch; ships no file with this licence's text; authors: Steven Sheldon
- `dlib` (MIT): https://github.com/elinorbgr/dlib
  - Copyright (c) 2015 Victor Berger
- `dpi` (Apache-2.0 AND MIT): https://github.com/rust-windowing/winit; ships no file with this licence's text; authors: none named (see its repository)
- `extended` (MIT): https://github.com/depp/extended-rs
  - Copyright 2022 Dietrich Epp
- `fontconfig-parser` (MIT): https://github.com/Riey/fontconfig-parser
  - Copyright (c) 2021 Riey
- `fontdb` (MIT): https://github.com/RazrFalcon/fontdb
  - Copyright (c) 2020 Yevhenii Reizner
- `gdk` (MIT): https://github.com/gtk-rs/gtk3-rs
- `gdk-pixbuf` (MIT): https://github.com/gtk-rs/gtk-rs-core
- `gdk-pixbuf-sys` (MIT): https://github.com/gtk-rs/gtk-rs-core
- `gdk-sys` (MIT): https://github.com/gtk-rs/gtk3-rs
- `gdkx11` (MIT): https://github.com/gtk-rs/gtk3-rs
- `gdkx11-sys` (MIT): https://github.com/gtk-rs/gtk3-rs
- `generic-array` (MIT): https://github.com/fizyk20/generic-array.git
  - Copyright (c) 2015 Bartłomiej Kamiński
- `gio` (MIT): https://github.com/gtk-rs/gtk-rs-core
- `gio-sys` (MIT): https://github.com/gtk-rs/gtk-rs-core
- `glib` (MIT): https://github.com/gtk-rs/gtk-rs-core
- `glib-macros` (MIT): https://github.com/gtk-rs/gtk-rs-core
- `glib-sys` (MIT): https://github.com/gtk-rs/gtk-rs-core
- `gobject-sys` (MIT): https://github.com/gtk-rs/gtk-rs-core
- `gtk` (MIT): https://github.com/gtk-rs/gtk3-rs
- `gtk-sys` (MIT): https://github.com/gtk-rs/gtk3-rs
- `gtk3-macros` (MIT): https://github.com/gtk-rs/gtk3-rs
- `include_dir` (MIT): https://github.com/Michael-F-Bryan/include_dir; ships no file with this licence's text; authors: Michael Bryan <michaelfbryan@gmail.com>
- `include_dir_macros` (MIT): https://github.com/Michael-F-Bryan/include_dir; ships no file with this licence's text; authors: Michael Bryan <michaelfbryan@gmail.com>
- `javascriptcore-rs` (MIT): https://github.com/tauri-apps/javascriptcore-rs
  - Copyright (c) 2013-2021, The Gtk-rs Project Developers.
  - Copyright (c) 2021, Tauri Programme within The Commons Conservancy.
- `javascriptcore-rs-sys` (MIT): https://github.com/tauri-apps/javascriptcore-rs
  - Copyright (c) 2013-2017, The Gtk-rs Project Developers.
- `jiff` (Unlicense OR MIT): https://github.com/BurntSushi/jiff
  - Copyright (c) 2015 Andrew Gallant
- `jiff-core` (Unlicense OR MIT): https://github.com/BurntSushi/jiff
  - Copyright (c) 2015 Andrew Gallant
- `json5` (MIT): https://github.com/callum-oakley/json5-rs
  - Copyright (c) 2025 Callum Oakley
- `libm` (MIT): https://github.com/rust-lang/compiler-builtins; ships no file with this licence's text; authors: Alex Crichton <alex@alexcrichton.com>, Amanieu d'Antras <amanieu@gmail.com>, Jorge Aparicio <japaricious@gmail.com>, Trevor Gross <tg@trevorgross.com>
- `libudev-sys` (MIT): https://github.com/dcuddeback/libudev-sys
  - Copyright (c) 2015 David Cuddeback
- `lua-src` (MIT): https://github.com/mlua-rs/lua-src-rs; ships no file with this licence's text; authors: Aleksandr Orlenko <zxteam@protonmail.com>
- `luajit-src` (MIT): https://github.com/mlua-rs/luajit-src-rs
  - Copyright (c) 2020-2025 Aleksandr Orlenko
- `memchr` (Unlicense OR MIT): https://github.com/BurntSushi/memchr
  - Copyright (c) 2015 Andrew Gallant
- `memoffset` (MIT): https://github.com/Gilnaa/memoffset
  - Copyright (c) 2017 Gilad Naaman
- `mlua-sys` (MIT): https://github.com/mlua-rs/mlua; ships no file with this licence's text; authors: Aleksandr Orlenko <zxteam@pm.me>
- `new_debug_unreachable` (MIT): https://github.com/mbrubeck/rust-debug-unreachable
  - Copyright (c) 2015 Jonathan Reem
- `nix` (MIT): https://github.com/nix-rust/nix
  - Copyright (c) 2015 Carl Lerche + nix-rust Authors
- `objc-sys` (MIT): https://github.com/madsmtm/objc2; ships no file with this licence's text; authors: Mads Marquart <mads@marquart.dk>
- `objc2` (MIT): https://github.com/madsmtm/objc2; ships no file with this licence's text; authors: Mads Marquart <mads@marquart.dk>, Steven Sheldon
- `objc2-app-kit` (MIT): https://github.com/madsmtm/objc2; ships no file with this licence's text; authors: none named (see its repository)
- `objc2-encode` (MIT): https://github.com/madsmtm/objc2; ships no file with this licence's text; authors: Mads Marquart <mads@marquart.dk>
- `objc2-foundation` (MIT): https://github.com/madsmtm/objc2; ships no file with this licence's text; authors: none named (see its repository)
- `objc2-ui-kit` (MIT): https://github.com/madsmtm/objc2; ships no file with this licence's text; authors: none named (see its repository)
- `ordered-float` (MIT): https://github.com/reem/rust-ordered-float
  - Copyright (c) 2015 Jonathan Reem
- `pango` (MIT): https://github.com/gtk-rs/gtk-rs-core
- `pango-sys` (MIT): https://github.com/gtk-rs/gtk-rs-core
- `phf` (MIT): https://github.com/rust-phf/rust-phf
  - Copyright (c) 2014-2022 Steven Fackler, Yuki Okushi
- `phf_codegen` (MIT): https://github.com/rust-phf/rust-phf
  - Copyright (c) 2014-2022 Steven Fackler, Yuki Okushi
- `phf_generator` (MIT): https://github.com/rust-phf/rust-phf
  - Copyright (c) 2014-2022 Steven Fackler, Yuki Okushi
- `phf_macros` (MIT): https://github.com/rust-phf/rust-phf
  - Copyright (c) 2014-2022 Steven Fackler, Yuki Okushi
- `phf_shared` (MIT): https://github.com/rust-phf/rust-phf
  - Copyright (c) 2014-2022 Steven Fackler, Yuki Okushi
- `precomputed-hash` (MIT): https://github.com/emilio/precomputed-hash
  - Copyright (c) 2017 Emilio Cobos Álvarez
- `quick-xml` (MIT): https://github.com/tafia/quick-xml
  - Copyright (c) 2016 Johann Tuffe
- `rfd` (MIT): https://github.com/PolyMeilex/rfd
  - Copyright (c) 2020 Bilal Elmoussaoui
  - Copyright (c) 2022 Bartłomiej Maryńczak
- `rmp` (MIT): https://github.com/3Hren/msgpack-rust
  - Copyright (c) 2017 Evgeny Safronov
- `rmp-serde` (MIT): https://github.com/3Hren/msgpack-rust
  - Copyright (c) 2017 Evgeny Safronov
- `same-file` (Unlicense OR MIT): https://github.com/BurntSushi/same-file
  - Copyright (c) 2017 Andrew Gallant
- `schannel` (MIT): https://github.com/steffengy/schannel-rs
  - Copyright (c) 2015 steffengy
- `sctk-adwaita` (MIT): https://github.com/PolyMeilex/sctk-adwaita
  - Copyright (c) 2022 Bartłomiej Maryńczak
- `serde-value` (MIT): https://github.com/arcnmx/serde-value
  - Copyright (c) 2016 arcnmx
- `simd-adler32` (MIT): https://github.com/mcountryman/simd-adler32
  - Copyright (c) [2021] [Marvin Countryman]
- `slab` (MIT): https://github.com/tokio-rs/slab
  - Copyright (c) 2019 Carl Lerche
- `smithay-client-toolkit` (MIT): https://github.com/smithay/client-toolkit
  - Copyright (c) 2018 Victor Berger
- `soup3` (MIT): https://gitlab.gnome.org/World/Rust/soup3-rs
  - Copyright (c) 2013-2017, The Gtk-rs Project Developers.
- `soup3-sys` (MIT): https://gitlab.gnome.org/World/Rust/soup3-rs
  - Copyright (c) 2013-2017, The Gtk-rs Project Developers.
- `spin` (MIT): https://github.com/mvdnes/spin-rs.git
  - Copyright (c) 2014 Mathijs van de Nes
- `strict-num` (MIT): https://github.com/RazrFalcon/strict-num
  - Copyright (c) 2022 Yevhenii Reizner
- `synstructure` (MIT): https://github.com/mystor/synstructure
  - Copyright 2016 Nika Layzell
- `tracing` (MIT): https://github.com/tokio-rs/tracing
  - Copyright (c) 2019 Tokio Contributors
- `tracing-core` (MIT): https://github.com/tokio-rs/tracing
  - Copyright (c) 2019 Tokio Contributors
  - Copyright (c) 2014 Mathijs van de Nes
- `version-compare` (MIT): https://gitlab.com/timvisee/version-compare
  - Copyright (c) 2017 Tim Visée
- `walkdir` (Unlicense OR MIT): https://github.com/BurntSushi/walkdir
  - Copyright (c) 2015 Andrew Gallant
- `wasapi` (MIT): https://github.com/HEnquist/wasapi-rs
  - Copyright (c) 2020 Henrik Enquist
- `wayland-backend` (MIT): https://github.com/smithay/wayland-rs
  - Copyright (c) 2015 Elinor Berger
- `wayland-client` (MIT): https://github.com/smithay/wayland-rs
  - Copyright (c) 2015 Elinor Berger
- `wayland-csd-frame` (MIT): https://github.com/rust-windowing/wayland-csd-frame
  - Copyright (c) 2023 Kirill Chibisov
- `wayland-cursor` (MIT): https://github.com/smithay/wayland-rs
  - Copyright (c) 2015 Elinor Berger
- `wayland-protocols` (MIT): https://github.com/smithay/wayland-rs
  - Copyright (c) 2015 Elinor Berger
- `wayland-protocols-plasma` (MIT): https://github.com/smithay/wayland-rs
  - Copyright (c) 2015 Elinor Berger
- `wayland-protocols-wlr` (MIT): https://github.com/smithay/wayland-rs
  - Copyright (c) 2015 Elinor Berger
- `wayland-scanner` (MIT): https://github.com/smithay/wayland-rs
  - Copyright (c) 2015 Elinor Berger
- `wayland-sys` (MIT): https://github.com/smithay/wayland-rs
  - Copyright (c) 2015 Elinor Berger
- `webkit2gtk` (MIT): https://github.com/tauri-apps/webkit2gtk-rs
  - Copyright (c) 2016 Boucher, Antoni <bouanto@zoho.com>
  - Copyright (c) 2017-2021, The Gtk-rs Project Developers.
  - Copyright (c) 2021, Tauri Programme within The Commons Conservancy
- `webkit2gtk-sys` (MIT): https://github.com/tauri-apps/webkit2gtk-rs
  - Copyright (c) 2016 Boucher, Antoni <bouanto@zoho.com>
- `webview2-com` (MIT): https://github.com/wravery/webview2-rs; ships no file with this licence's text; authors: none named (see its repository)
- `webview2-com-macros` (MIT): https://github.com/wravery/webview2-rs; ships no file with this licence's text; authors: none named (see its repository)
- `webview2-com-sys` (MIT): https://github.com/wravery/webview2-rs; ships no file with this licence's text; authors: none named (see its repository)
- `which` (MIT): https://github.com/harryfei/which-rs.git
  - Copyright (c) 2015 fangyuanziti
- `winapi-util` (Unlicense OR MIT): https://github.com/BurntSushi/winapi-util
  - Copyright (c) 2017 Andrew Gallant
- `winnow` (MIT): https://github.com/winnow-rs/winnow
- `x11` (MIT): https://github.com/AltF02/x11-rs.git
- `x11-dl` (MIT): https://github.com/AltF02/x11-rs.git
- `xcursor` (MIT): https://github.com/esposm03/xcursor-rs
  - Copyright (c) 2020 Samuele Esposito
- `xkbcommon-dl` (MIT): https://github.com/rust-windowing/xkbcommon-dl
  - Copyright (c) 2023 Kirill Chibisov
- `xml-rs` (MIT): https://github.com/kornelski/xml-rs
  - Copyright (c) 2014 Vladimir Matveev
- `zip` (MIT): https://github.com/zip-rs/zip2
  - Copyright (c) 2014 Mathijs van de Nes
- `zmij` (MIT): https://github.com/dtolnay/zmij

## MIT License (`MIT`)

````text
# 3rd Party Notices

The `cfg_aliases!` macro uses a lot of the code from [`tectonic_cfg_support::target_cfg!`] macro which is under the following license:

[`tectonic_cfg_support::target_cfg!`]: https://github.com/tectonic-typesetting/tectonic/blob/f2439b936470ad27bdf92882064bc4702ee01899/cfg_support/src/lib.rs#L166

    tectonic_cfg_support is licensed under the MIT License.

    Permission is hereby granted, free of charge, to any person obtaining a copy
    of this software and associated documentation files (the “Software”), to deal
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
---
````

- `cfg_aliases` (MIT): https://github.com/katharostech/cfg_aliases

## MIT License (`MIT`)

````text
MIT License

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

---

This project contains portions of code and architectural concepts originally
derived from the "nipper" project (https://github.com/importcjj/nipper),
developed by Chen Jiaju, licensed under the MIT License and the Apache License 2.0 (dual licensed).
````

- `dom_query` (MIT): https://github.com/niklak/dom_query
  - Copyright (c) 2023 Mykola Humanov

## MIT License (`MIT`)

````text
Both mlua and rlua are distributed under the MIT license, which is reproduced
below:

MIT License

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
````

- `mlua` (MIT): https://github.com/mlua-rs/mlua
  - Copyright (c) 2019-2021 A. Orlenko
  - Copyright (c) 2017 rlua

## MIT License (`MIT`)

````text
Permission is hereby granted, free of charge, to any person obtaining a
copy of this software and associated documentation files (the "Software"),
to deal in the Software without restriction, including without limitation
the rights to use, copy, modify, merge, publish, distribute, sublicense,
and/or sell copies of the Software, and to permit persons to whom the
Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice (including the next
paragraph) shall be included in all copies or substantial portions of the
Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.  IN NO EVENT SHALL
THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.

---

The above is the version of the MIT "Expat" License used by X.org:

    http://cgit.freedesktop.org/xorg/xserver/tree/COPYING
````

- `wayland-protocols` (MIT): https://github.com/smithay/wayland-rs
  - Copyright © 2008-2013 Kristian Høgsberg
  - Copyright © 2010-2013 Intel Corporation
  - Copyright © 2013      Rafael Antognolli
  - Copyright © 2013      Jasper St. Pierre
  - Copyright © 2014      Jonas Ådahl
  - Copyright © 2014      Jason Ekstrand
  - Copyright © 2014-2015 Collabora, Ltd.
  - Copyright © 2015      Red Hat Inc.

## MIT License (`MIT`)

````text
MIT License Copyright (c) <year> <copyright holders>

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is furnished
to do so, subject to the following conditions:

The above copyright notice and this permission notice (including the next
paragraph) shall be included in all copies or substantial portions of the
Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS
FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS
OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY,
WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF
OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
````

- `wayland-protocols-plasma` (MIT): https://github.com/smithay/wayland-rs

## Mozilla Public License 2.0 (`MPL-2.0`)

These crates are used unmodified; their source code is on crates.io and in the repository listed.

````text
Mozilla Public License Version 2.0
==================================

1. Definitions
--------------

1.1. "Contributor"
    means each individual or legal entity that creates, contributes to
    the creation of, or owns Covered Software.

1.2. "Contributor Version"
    means the combination of the Contributions of others (if any) used
    by a Contributor and that particular Contributor's Contribution.

1.3. "Contribution"
    means Covered Software of a particular Contributor.

1.4. "Covered Software"
    means Source Code Form to which the initial Contributor has attached
    the notice in Exhibit A, the Executable Form of such Source Code
    Form, and Modifications of such Source Code Form, in each case
    including portions thereof.

1.5. "Incompatible With Secondary Licenses"
    means

    (a) that the initial Contributor has attached the notice described
        in Exhibit B to the Covered Software; or

    (b) that the Covered Software was made available under the terms of
        version 1.1 or earlier of the License, but not also under the
        terms of a Secondary License.

1.6. "Executable Form"
    means any form of the work other than Source Code Form.

1.7. "Larger Work"
    means a work that combines Covered Software with other material, in
    a separate file or files, that is not Covered Software.

1.8. "License"
    means this document.

1.9. "Licensable"
    means having the right to grant, to the maximum extent possible,
    whether at the time of the initial grant or subsequently, any and
    all of the rights conveyed by this License.

1.10. "Modifications"
    means any of the following:

    (a) any file in Source Code Form that results from an addition to,
        deletion from, or modification of the contents of Covered
        Software; or

    (b) any new file in Source Code Form that contains any Covered
        Software.

1.11. "Patent Claims" of a Contributor
    means any patent claim(s), including without limitation, method,
    process, and apparatus claims, in any patent Licensable by such
    Contributor that would be infringed, but for the grant of the
    License, by the making, using, selling, offering for sale, having
    made, import, or transfer of either its Contributions or its
    Contributor Version.

1.12. "Secondary License"
    means either the GNU General Public License, Version 2.0, the GNU
    Lesser General Public License, Version 2.1, the GNU Affero General
    Public License, Version 3.0, or any later versions of those
    licenses.

1.13. "Source Code Form"
    means the form of the work preferred for making modifications.

1.14. "You" (or "Your")
    means an individual or a legal entity exercising rights under this
    License. For legal entities, "You" includes any entity that
    controls, is controlled by, or is under common control with You. For
    purposes of this definition, "control" means (a) the power, direct
    or indirect, to cause the direction or management of such entity,
    whether by contract or otherwise, or (b) ownership of more than
    fifty percent (50%) of the outstanding shares or beneficial
    ownership of such entity.

2. License Grants and Conditions
--------------------------------

2.1. Grants

Each Contributor hereby grants You a world-wide, royalty-free,
non-exclusive license:

(a) under intellectual property rights (other than patent or trademark)
    Licensable by such Contributor to use, reproduce, make available,
    modify, display, perform, distribute, and otherwise exploit its
    Contributions, either on an unmodified basis, with Modifications, or
    as part of a Larger Work; and

(b) under Patent Claims of such Contributor to make, use, sell, offer
    for sale, have made, import, and otherwise transfer either its
    Contributions or its Contributor Version.

2.2. Effective Date

The licenses granted in Section 2.1 with respect to any Contribution
become effective for each Contribution on the date the Contributor first
distributes such Contribution.

2.3. Limitations on Grant Scope

The licenses granted in this Section 2 are the only rights granted under
this License. No additional rights or licenses will be implied from the
distribution or licensing of Covered Software under this License.
Notwithstanding Section 2.1(b) above, no patent license is granted by a
Contributor:

(a) for any code that a Contributor has removed from Covered Software;
    or

(b) for infringements caused by: (i) Your and any other third party's
    modifications of Covered Software, or (ii) the combination of its
    Contributions with other software (except as part of its Contributor
    Version); or

(c) under Patent Claims infringed by Covered Software in the absence of
    its Contributions.

This License does not grant any rights in the trademarks, service marks,
or logos of any Contributor (except as may be necessary to comply with
the notice requirements in Section 3.4).

2.4. Subsequent Licenses

No Contributor makes additional grants as a result of Your choice to
distribute the Covered Software under a subsequent version of this
License (see Section 10.2) or under the terms of a Secondary License (if
permitted under the terms of Section 3.3).

2.5. Representation

Each Contributor represents that the Contributor believes its
Contributions are its original creation(s) or it has sufficient rights
to grant the rights to its Contributions conveyed by this License.

2.6. Fair Use

This License is not intended to limit any rights You have under
applicable copyright doctrines of fair use, fair dealing, or other
equivalents.

2.7. Conditions

Sections 3.1, 3.2, 3.3, and 3.4 are conditions of the licenses granted
in Section 2.1.

3. Responsibilities
-------------------

3.1. Distribution of Source Form

All distribution of Covered Software in Source Code Form, including any
Modifications that You create or to which You contribute, must be under
the terms of this License. You must inform recipients that the Source
Code Form of the Covered Software is governed by the terms of this
License, and how they can obtain a copy of this License. You may not
attempt to alter or restrict the recipients' rights in the Source Code
Form.

3.2. Distribution of Executable Form

If You distribute Covered Software in Executable Form then:

(a) such Covered Software must also be made available in Source Code
    Form, as described in Section 3.1, and You must inform recipients of
    the Executable Form how they can obtain a copy of such Source Code
    Form by reasonable means in a timely manner, at a charge no more
    than the cost of distribution to the recipient; and

(b) You may distribute such Executable Form under the terms of this
    License, or sublicense it under different terms, provided that the
    license for the Executable Form does not attempt to limit or alter
    the recipients' rights in the Source Code Form under this License.

3.3. Distribution of a Larger Work

You may create and distribute a Larger Work under terms of Your choice,
provided that You also comply with the requirements of this License for
the Covered Software. If the Larger Work is a combination of Covered
Software with a work governed by one or more Secondary Licenses, and the
Covered Software is not Incompatible With Secondary Licenses, this
License permits You to additionally distribute such Covered Software
under the terms of such Secondary License(s), so that the recipient of
the Larger Work may, at their option, further distribute the Covered
Software under the terms of either this License or such Secondary
License(s).

3.4. Notices

You may not remove or alter the substance of any license notices
(including copyright notices, patent notices, disclaimers of warranty,
or limitations of liability) contained within the Source Code Form of
the Covered Software, except that You may alter any license notices to
the extent required to remedy known factual inaccuracies.

3.5. Application of Additional Terms

You may choose to offer, and to charge a fee for, warranty, support,
indemnity or liability obligations to one or more recipients of Covered
Software. However, You may do so only on Your own behalf, and not on
behalf of any Contributor. You must make it absolutely clear that any
such warranty, support, indemnity, or liability obligation is offered by
You alone, and You hereby agree to indemnify every Contributor for any
liability incurred by such Contributor as a result of warranty, support,
indemnity or liability terms You offer. You may include additional
disclaimers of warranty and limitations of liability specific to any
jurisdiction.

4. Inability to Comply Due to Statute or Regulation
---------------------------------------------------

If it is impossible for You to comply with any of the terms of this
License with respect to some or all of the Covered Software due to
statute, judicial order, or regulation then You must: (a) comply with
the terms of this License to the maximum extent possible; and (b)
describe the limitations and the code they affect. Such description must
be placed in a text file included with all distributions of the Covered
Software under this License. Except to the extent prohibited by statute
or regulation, such description must be sufficiently detailed for a
recipient of ordinary skill to be able to understand it.

5. Termination
--------------

5.1. The rights granted under this License will terminate automatically
if You fail to comply with any of its terms. However, if You become
compliant, then the rights granted under this License from a particular
Contributor are reinstated (a) provisionally, unless and until such
Contributor explicitly and finally terminates Your grants, and (b) on an
ongoing basis, if such Contributor fails to notify You of the
non-compliance by some reasonable means prior to 60 days after You have
come back into compliance. Moreover, Your grants from a particular
Contributor are reinstated on an ongoing basis if such Contributor
notifies You of the non-compliance by some reasonable means, this is the
first time You have received notice of non-compliance with this License
from such Contributor, and You become compliant prior to 30 days after
Your receipt of the notice.

5.2. If You initiate litigation against any entity by asserting a patent
infringement claim (excluding declaratory judgment actions,
counter-claims, and cross-claims) alleging that a Contributor Version
directly or indirectly infringes any patent, then the rights granted to
You by any and all Contributors for the Covered Software under Section
2.1 of this License shall terminate.

5.3. In the event of termination under Sections 5.1 or 5.2 above, all
end user license agreements (excluding distributors and resellers) which
have been validly granted by You or Your distributors under this License
prior to termination shall survive termination.

************************************************************************
*                                                                      *
*  6. Disclaimer of Warranty                                           *
*  -------------------------                                           *
*                                                                      *
*  Covered Software is provided under this License on an "as is"       *
*  basis, without warranty of any kind, either expressed, implied, or  *
*  statutory, including, without limitation, warranties that the       *
*  Covered Software is free of defects, merchantable, fit for a        *
*  particular purpose or non-infringing. The entire risk as to the     *
*  quality and performance of the Covered Software is with You.        *
*  Should any Covered Software prove defective in any respect, You     *
*  (not any Contributor) assume the cost of any necessary servicing,   *
*  repair, or correction. This disclaimer of warranty constitutes an   *
*  essential part of this License. No use of any Covered Software is   *
*  authorized under this License except under this disclaimer.         *
*                                                                      *
************************************************************************

************************************************************************
*                                                                      *
*  7. Limitation of Liability                                          *
*  --------------------------                                          *
*                                                                      *
*  Under no circumstances and under no legal theory, whether tort      *
*  (including negligence), contract, or otherwise, shall any           *
*  Contributor, or anyone who distributes Covered Software as          *
*  permitted above, be liable to You for any direct, indirect,         *
*  special, incidental, or consequential damages of any character      *
*  including, without limitation, damages for lost profits, loss of    *
*  goodwill, work stoppage, computer failure or malfunction, or any    *
*  and all other commercial damages or losses, even if such party      *
*  shall have been informed of the possibility of such damages. This   *
*  limitation of liability shall not apply to liability for death or   *
*  personal injury resulting from such party's negligence to the       *
*  extent applicable law prohibits such limitation. Some               *
*  jurisdictions do not allow the exclusion or limitation of           *
*  incidental or consequential damages, so this exclusion and          *
*  limitation may not apply to You.                                    *
*                                                                      *
************************************************************************

8. Litigation
-------------

Any litigation relating to this License may be brought only in the
courts of a jurisdiction where the defendant maintains its principal
place of business and such litigation shall be governed by laws of that
jurisdiction, without reference to its conflict-of-law provisions.
Nothing in this Section shall prevent a party's ability to bring
cross-claims or counter-claims.

9. Miscellaneous
----------------

This License represents the complete agreement concerning the subject
matter hereof. If any provision of this License is held to be
unenforceable, such provision shall be reformed only to the extent
necessary to make it enforceable. Any law or regulation which provides
that the language of a contract shall be construed against the drafter
shall not be used to construe this License against a Contributor.

10. Versions of the License
---------------------------

10.1. New Versions

Mozilla Foundation is the license steward. Except as provided in Section
10.3, no one other than the license steward has the right to modify or
publish new versions of this License. Each version will be given a
distinguishing version number.

10.2. Effect of New Versions

You may distribute the Covered Software under the terms of the version
of the License under which You originally received the Covered Software,
or under the terms of any subsequent version published by the license
steward.

10.3. Modified Versions

If you create software not governed by this License, and you want to
create a new license for such software, you may create and use a
modified version of this License if you rename the license and remove
any references to the name of the license steward (except to note that
such modified license differs from this License).

10.4. Distributing Source Code Form that is Incompatible With Secondary
Licenses

If You choose to distribute Source Code Form that is Incompatible With
Secondary Licenses under the terms of this version of the License, the
notice described in Exhibit B of this License must be attached.

Exhibit A - Source Code Form License Notice
-------------------------------------------

  This Source Code Form is subject to the terms of the Mozilla Public
  License, v. 2.0. If a copy of the MPL was not distributed with this
  file, You can obtain one at http://mozilla.org/MPL/2.0/.

If it is not possible or desirable to put the notice in a particular
file, then You may include the notice in a location (such as a LICENSE
file in a relevant directory) where a recipient would be likely to look
for such a notice.

You may add additional accurate notices of copyright ownership.

Exhibit B - "Incompatible With Secondary Licenses" Notice
---------------------------------------------------------

  This Source Code Form is "Incompatible With Secondary Licenses", as
  defined by the Mozilla Public License, v. 2.0.
````

- `cssparser` (MPL-2.0): https://crates.io/crates/cssparser, https://github.com/servo/rust-cssparser
- `cssparser-macros` (MPL-2.0): https://crates.io/crates/cssparser-macros, https://github.com/servo/rust-cssparser
- `dtoa-short` (MPL-2.0): https://crates.io/crates/dtoa-short, https://github.com/upsuper/dtoa-short
- `symphonia` (MPL-2.0): https://crates.io/crates/symphonia, https://github.com/pdeljanov/Symphonia
- `symphonia-bundle-flac` (MPL-2.0): https://crates.io/crates/symphonia-bundle-flac, https://github.com/pdeljanov/Symphonia
- `symphonia-bundle-mp3` (MPL-2.0): https://crates.io/crates/symphonia-bundle-mp3, https://github.com/pdeljanov/Symphonia
- `symphonia-codec-adpcm` (MPL-2.0): https://crates.io/crates/symphonia-codec-adpcm, https://github.com/pdeljanov/Symphonia
- `symphonia-codec-pcm` (MPL-2.0): https://crates.io/crates/symphonia-codec-pcm, https://github.com/pdeljanov/Symphonia
- `symphonia-codec-vorbis` (MPL-2.0): https://crates.io/crates/symphonia-codec-vorbis, https://github.com/pdeljanov/Symphonia
- `symphonia-common` (MPL-2.0): https://crates.io/crates/symphonia-common, https://github.com/pdeljanov/Symphonia
- `symphonia-core` (MPL-2.0): https://crates.io/crates/symphonia-core, https://github.com/pdeljanov/Symphonia
- `symphonia-format-ogg` (MPL-2.0): https://crates.io/crates/symphonia-format-ogg, https://github.com/pdeljanov/Symphonia
- `symphonia-format-riff` (MPL-2.0): https://crates.io/crates/symphonia-format-riff, https://github.com/pdeljanov/Symphonia
- `symphonia-metadata` (MPL-2.0): https://crates.io/crates/symphonia-metadata, https://github.com/pdeljanov/Symphonia

## Mozilla Public License 2.0 (`MPL-2.0`)

These crates are used unmodified; their source code is on crates.io and in the repository listed.

````text
Mozilla Public License Version 2.0
==================================

1. Definitions
--------------

1.1. "Contributor"
    means each individual or legal entity that creates, contributes to
    the creation of, or owns Covered Software.

1.2. "Contributor Version"
    means the combination of the Contributions of others (if any) used
    by a Contributor and that particular Contributor's Contribution.

1.3. "Contribution"
    means Covered Software of a particular Contributor.

1.4. "Covered Software"
    means Source Code Form to which the initial Contributor has attached
    the notice in Exhibit A, the Executable Form of such Source Code
    Form, and Modifications of such Source Code Form, in each case
    including portions thereof.

1.5. "Incompatible With Secondary Licenses"
    means

    (a) that the initial Contributor has attached the notice described
        in Exhibit B to the Covered Software; or

    (b) that the Covered Software was made available under the terms of
        version 1.1 or earlier of the License, but not also under the
        terms of a Secondary License.

1.6. "Executable Form"
    means any form of the work other than Source Code Form.

1.7. "Larger Work"
    means a work that combines Covered Software with other material, in
    a separate file or files, that is not Covered Software.

1.8. "License"
    means this document.

1.9. "Licensable"
    means having the right to grant, to the maximum extent possible,
    whether at the time of the initial grant or subsequently, any and
    all of the rights conveyed by this License.

1.10. "Modifications"
    means any of the following:

    (a) any file in Source Code Form that results from an addition to,
        deletion from, or modification of the contents of Covered
        Software; or

    (b) any new file in Source Code Form that contains any Covered
        Software.

1.11. "Patent Claims" of a Contributor
    means any patent claim(s), including without limitation, method,
    process, and apparatus claims, in any patent Licensable by such
    Contributor that would be infringed, but for the grant of the
    License, by the making, using, selling, offering for sale, having
    made, import, or transfer of either its Contributions or its
    Contributor Version.

1.12. "Secondary License"
    means either the GNU General Public License, Version 2.0, the GNU
    Lesser General Public License, Version 2.1, the GNU Affero General
    Public License, Version 3.0, or any later versions of those
    licenses.

1.13. "Source Code Form"
    means the form of the work preferred for making modifications.

1.14. "You" (or "Your")
    means an individual or a legal entity exercising rights under this
    License. For legal entities, "You" includes any entity that
    controls, is controlled by, or is under common control with You. For
    purposes of this definition, "control" means (a) the power, direct
    or indirect, to cause the direction or management of such entity,
    whether by contract or otherwise, or (b) ownership of more than
    fifty percent (50%) of the outstanding shares or beneficial
    ownership of such entity.

2. License Grants and Conditions
--------------------------------

2.1. Grants

Each Contributor hereby grants You a world-wide, royalty-free,
non-exclusive license:

(a) under intellectual property rights (other than patent or trademark)
    Licensable by such Contributor to use, reproduce, make available,
    modify, display, perform, distribute, and otherwise exploit its
    Contributions, either on an unmodified basis, with Modifications, or
    as part of a Larger Work; and

(b) under Patent Claims of such Contributor to make, use, sell, offer
    for sale, have made, import, and otherwise transfer either its
    Contributions or its Contributor Version.

2.2. Effective Date

The licenses granted in Section 2.1 with respect to any Contribution
become effective for each Contribution on the date the Contributor first
distributes such Contribution.

2.3. Limitations on Grant Scope

The licenses granted in this Section 2 are the only rights granted under
this License. No additional rights or licenses will be implied from the
distribution or licensing of Covered Software under this License.
Notwithstanding Section 2.1(b) above, no patent license is granted by a
Contributor:

(a) for any code that a Contributor has removed from Covered Software;
    or

(b) for infringements caused by: (i) Your and any other third party's
    modifications of Covered Software, or (ii) the combination of its
    Contributions with other software (except as part of its Contributor
    Version); or

(c) under Patent Claims infringed by Covered Software in the absence of
    its Contributions.

This License does not grant any rights in the trademarks, service marks,
or logos of any Contributor (except as may be necessary to comply with
the notice requirements in Section 3.4).

2.4. Subsequent Licenses

No Contributor makes additional grants as a result of Your choice to
distribute the Covered Software under a subsequent version of this
License (see Section 10.2) or under the terms of a Secondary License (if
permitted under the terms of Section 3.3).

2.5. Representation

Each Contributor represents that the Contributor believes its
Contributions are its original creation(s) or it has sufficient rights
to grant the rights to its Contributions conveyed by this License.

2.6. Fair Use

This License is not intended to limit any rights You have under
applicable copyright doctrines of fair use, fair dealing, or other
equivalents.

2.7. Conditions

Sections 3.1, 3.2, 3.3, and 3.4 are conditions of the licenses granted
in Section 2.1.

3. Responsibilities
-------------------

3.1. Distribution of Source Form

All distribution of Covered Software in Source Code Form, including any
Modifications that You create or to which You contribute, must be under
the terms of this License. You must inform recipients that the Source
Code Form of the Covered Software is governed by the terms of this
License, and how they can obtain a copy of this License. You may not
attempt to alter or restrict the recipients' rights in the Source Code
Form.

3.2. Distribution of Executable Form

If You distribute Covered Software in Executable Form then:

(a) such Covered Software must also be made available in Source Code
    Form, as described in Section 3.1, and You must inform recipients of
    the Executable Form how they can obtain a copy of such Source Code
    Form by reasonable means in a timely manner, at a charge no more
    than the cost of distribution to the recipient; and

(b) You may distribute such Executable Form under the terms of this
    License, or sublicense it under different terms, provided that the
    license for the Executable Form does not attempt to limit or alter
    the recipients' rights in the Source Code Form under this License.

3.3. Distribution of a Larger Work

You may create and distribute a Larger Work under terms of Your choice,
provided that You also comply with the requirements of this License for
the Covered Software. If the Larger Work is a combination of Covered
Software with a work governed by one or more Secondary Licenses, and the
Covered Software is not Incompatible With Secondary Licenses, this
License permits You to additionally distribute such Covered Software
under the terms of such Secondary License(s), so that the recipient of
the Larger Work may, at their option, further distribute the Covered
Software under the terms of either this License or such Secondary
License(s).

3.4. Notices

You may not remove or alter the substance of any license notices
(including copyright notices, patent notices, disclaimers of warranty,
or limitations of liability) contained within the Source Code Form of
the Covered Software, except that You may alter any license notices to
the extent required to remedy known factual inaccuracies.

3.5. Application of Additional Terms

You may choose to offer, and to charge a fee for, warranty, support,
indemnity or liability obligations to one or more recipients of Covered
Software. However, You may do so only on Your own behalf, and not on
behalf of any Contributor. You must make it absolutely clear that any
such warranty, support, indemnity, or liability obligation is offered by
You alone, and You hereby agree to indemnify every Contributor for any
liability incurred by such Contributor as a result of warranty, support,
indemnity or liability terms You offer. You may include additional
disclaimers of warranty and limitations of liability specific to any
jurisdiction.

4. Inability to Comply Due to Statute or Regulation
---------------------------------------------------

If it is impossible for You to comply with any of the terms of this
License with respect to some or all of the Covered Software due to
statute, judicial order, or regulation then You must: (a) comply with
the terms of this License to the maximum extent possible; and (b)
describe the limitations and the code they affect. Such description must
be placed in a text file included with all distributions of the Covered
Software under this License. Except to the extent prohibited by statute
or regulation, such description must be sufficiently detailed for a
recipient of ordinary skill to be able to understand it.

5. Termination
--------------

5.1. The rights granted under this License will terminate automatically
if You fail to comply with any of its terms. However, if You become
compliant, then the rights granted under this License from a particular
Contributor are reinstated (a) provisionally, unless and until such
Contributor explicitly and finally terminates Your grants, and (b) on an
ongoing basis, if such Contributor fails to notify You of the
non-compliance by some reasonable means prior to 60 days after You have
come back into compliance. Moreover, Your grants from a particular
Contributor are reinstated on an ongoing basis if such Contributor
notifies You of the non-compliance by some reasonable means, this is the
first time You have received notice of non-compliance with this License
from such Contributor, and You become compliant prior to 30 days after
Your receipt of the notice.

5.2. If You initiate litigation against any entity by asserting a patent
infringement claim (excluding declaratory judgment actions,
counter-claims, and cross-claims) alleging that a Contributor Version
directly or indirectly infringes any patent, then the rights granted to
You by any and all Contributors for the Covered Software under Section
2.1 of this License shall terminate.

5.3. In the event of termination under Sections 5.1 or 5.2 above, all
end user license agreements (excluding distributors and resellers) which
have been validly granted by You or Your distributors under this License
prior to termination shall survive termination.

************************************************************************
*                                                                      *
*  6. Disclaimer of Warranty                                           *
*  -------------------------                                           *
*                                                                      *
*  Covered Software is provided under this License on an "as is"       *
*  basis, without warranty of any kind, either expressed, implied, or  *
*  statutory, including, without limitation, warranties that the       *
*  Covered Software is free of defects, merchantable, fit for a        *
*  particular purpose or non-infringing. The entire risk as to the     *
*  quality and performance of the Covered Software is with You.        *
*  Should any Covered Software prove defective in any respect, You     *
*  (not any Contributor) assume the cost of any necessary servicing,   *
*  repair, or correction. This disclaimer of warranty constitutes an   *
*  essential part of this License. No use of any Covered Software is   *
*  authorized under this License except under this disclaimer.         *
*                                                                      *
************************************************************************

************************************************************************
*                                                                      *
*  7. Limitation of Liability                                          *
*  --------------------------                                          *
*                                                                      *
*  Under no circumstances and under no legal theory, whether tort      *
*  (including negligence), contract, or otherwise, shall any           *
*  Contributor, or anyone who distributes Covered Software as          *
*  permitted above, be liable to You for any direct, indirect,         *
*  special, incidental, or consequential damages of any character      *
*  including, without limitation, damages for lost profits, loss of    *
*  goodwill, work stoppage, computer failure or malfunction, or any    *
*  and all other commercial damages or losses, even if such party      *
*  shall have been informed of the possibility of such damages. This   *
*  limitation of liability shall not apply to liability for death or   *
*  personal injury resulting from such party's negligence to the       *
*  extent applicable law prohibits such limitation. Some               *
*  jurisdictions do not allow the exclusion or limitation of           *
*  incidental or consequential damages, so this exclusion and          *
*  limitation may not apply to You.                                    *
*                                                                      *
************************************************************************

8. Litigation
-------------

Any litigation relating to this License may be brought only in the
courts of a jurisdiction where the defendant maintains its principal
place of business and such litigation shall be governed by laws of that
jurisdiction, without reference to its conflict-of-law provisions.
Nothing in this Section shall prevent a party's ability to bring
cross-claims or counter-claims.

9. Miscellaneous
----------------

This License represents the complete agreement concerning the subject
matter hereof. If any provision of this License is held to be
unenforceable, such provision shall be reformed only to the extent
necessary to make it enforceable. Any law or regulation which provides
that the language of a contract shall be construed against the drafter
shall not be used to construe this License against a Contributor.

10. Versions of the License
---------------------------

10.1. New Versions

Mozilla Foundation is the license steward. Except as provided in Section
10.3, no one other than the license steward has the right to modify or
publish new versions of this License. Each version will be given a
distinguishing version number.

10.2. Effect of New Versions

You may distribute the Covered Software under the terms of the version
of the License under which You originally received the Covered Software,
or under the terms of any subsequent version published by the license
steward.

10.3. Modified Versions

If you create software not governed by this License, and you want to
create a new license for such software, you may create and use a
modified version of this License if you rename the license and remove
any references to the name of the license steward (except to note that
such modified license differs from this License).

10.4. Distributing Source Code Form that is Incompatible With Secondary
Licenses

If You choose to distribute Source Code Form that is Incompatible With
Secondary Licenses under the terms of this version of the License, the
notice described in Exhibit B of this License must be attached.

Exhibit A - Source Code Form License Notice
-------------------------------------------

  This Source Code Form is subject to the terms of the Mozilla Public
  License, v. 2.0. If a copy of the MPL was not distributed with this
  file, You can obtain one at https://mozilla.org/MPL/2.0/.

If it is not possible or desirable to put the notice in a particular
file, then You may include the notice in a location (such as a LICENSE
file in a relevant directory) where a recipient would be likely to look
for such a notice.

You may add additional accurate notices of copyright ownership.

Exhibit B - "Incompatible With Secondary Licenses" Notice
---------------------------------------------------------

  This Source Code Form is "Incompatible With Secondary Licenses", as
  defined by the Mozilla Public License, v. 2.0.
````

- `option-ext` (MPL-2.0): https://crates.io/crates/option-ext, https://github.com/soc/option-ext.git
- `selectors` (MPL-2.0): https://crates.io/crates/selectors, https://github.com/servo/stylo

## Unicode License v3 (`Unicode-3.0`)

````text
UNICODE LICENSE V3

COPYRIGHT AND PERMISSION NOTICE

Copyright © 2020-2024 Unicode, Inc.

NOTICE TO USER: Carefully read the following legal agreement. BY
DOWNLOADING, INSTALLING, COPYING OR OTHERWISE USING DATA FILES, AND/OR
SOFTWARE, YOU UNEQUIVOCALLY ACCEPT, AND AGREE TO BE BOUND BY, ALL OF THE
TERMS AND CONDITIONS OF THIS AGREEMENT. IF YOU DO NOT AGREE, DO NOT
DOWNLOAD, INSTALL, COPY, DISTRIBUTE OR USE THE DATA FILES OR SOFTWARE.

Permission is hereby granted, free of charge, to any person obtaining a
copy of data files and any associated documentation (the "Data Files") or
software and any associated documentation (the "Software") to deal in the
Data Files or Software without restriction, including without limitation
the rights to use, copy, modify, merge, publish, distribute, and/or sell
copies of the Data Files or Software, and to permit persons to whom the
Data Files or Software are furnished to do so, provided that either (a)
this copyright and permission notice appear with all copies of the Data
Files or Software, or (b) this copyright and permission notice appear in
associated Documentation.

THE DATA FILES AND SOFTWARE ARE PROVIDED "AS IS", WITHOUT WARRANTY OF ANY
KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT OF
THIRD PARTY RIGHTS.

IN NO EVENT SHALL THE COPYRIGHT HOLDER OR HOLDERS INCLUDED IN THIS NOTICE
BE LIABLE FOR ANY CLAIM, OR ANY SPECIAL INDIRECT OR CONSEQUENTIAL DAMAGES,
OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS,
WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION,
ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THE DATA
FILES OR SOFTWARE.

Except as contained in this notice, the name of a copyright holder shall
not be used in advertising or otherwise to promote the sale, use or other
dealings in these Data Files or Software without prior written
authorization of the copyright holder.

SPDX-License-Identifier: Unicode-3.0

—

Portions of ICU4X may have been adapted from ICU4C and/or ICU4J.
ICU 1.8.1 to ICU 57.1 © 1995-2016 International Business Machines Corporation and others.
````

- `icu_collections` (Unicode-3.0): https://github.com/unicode-org/icu4x
- `icu_locale_core` (Unicode-3.0): https://github.com/unicode-org/icu4x
- `icu_normalizer` (Unicode-3.0): https://github.com/unicode-org/icu4x
- `icu_normalizer_data` (Unicode-3.0): https://github.com/unicode-org/icu4x
- `icu_properties` (Unicode-3.0): https://github.com/unicode-org/icu4x
- `icu_properties_data` (Unicode-3.0): https://github.com/unicode-org/icu4x
- `icu_provider` (Unicode-3.0): https://github.com/unicode-org/icu4x
- `litemap` (Unicode-3.0): https://github.com/unicode-org/icu4x
- `potential_utf` (Unicode-3.0): https://github.com/unicode-org/icu4x
- `tinystr` (Unicode-3.0): https://github.com/unicode-org/icu4x
- `writeable` (Unicode-3.0): https://github.com/unicode-org/icu4x
- `yoke` (Unicode-3.0): https://github.com/unicode-org/icu4x
- `yoke-derive` (Unicode-3.0): https://github.com/unicode-org/icu4x
- `zerofrom` (Unicode-3.0): https://github.com/unicode-org/icu4x
- `zerofrom-derive` (Unicode-3.0): https://github.com/unicode-org/icu4x
- `zerotrie` (Unicode-3.0): https://github.com/unicode-org/icu4x
- `zerovec` (Unicode-3.0): https://github.com/unicode-org/icu4x
- `zerovec-derive` (Unicode-3.0): https://github.com/unicode-org/icu4x

## Unicode License v3 (`Unicode-3.0`)

````text
UNICODE LICENSE V3

COPYRIGHT AND PERMISSION NOTICE

Copyright © 1991-2023 Unicode, Inc.

NOTICE TO USER: Carefully read the following legal agreement. BY
DOWNLOADING, INSTALLING, COPYING OR OTHERWISE USING DATA FILES, AND/OR
SOFTWARE, YOU UNEQUIVOCALLY ACCEPT, AND AGREE TO BE BOUND BY, ALL OF THE
TERMS AND CONDITIONS OF THIS AGREEMENT. IF YOU DO NOT AGREE, DO NOT
DOWNLOAD, INSTALL, COPY, DISTRIBUTE OR USE THE DATA FILES OR SOFTWARE.

Permission is hereby granted, free of charge, to any person obtaining a
copy of data files and any associated documentation (the "Data Files") or
software and any associated documentation (the "Software") to deal in the
Data Files or Software without restriction, including without limitation
the rights to use, copy, modify, merge, publish, distribute, and/or sell
copies of the Data Files or Software, and to permit persons to whom the
Data Files or Software are furnished to do so, provided that either (a)
this copyright and permission notice appear with all copies of the Data
Files or Software, or (b) this copyright and permission notice appear in
associated Documentation.

THE DATA FILES AND SOFTWARE ARE PROVIDED "AS IS", WITHOUT WARRANTY OF ANY
KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT OF
THIRD PARTY RIGHTS.

IN NO EVENT SHALL THE COPYRIGHT HOLDER OR HOLDERS INCLUDED IN THIS NOTICE
BE LIABLE FOR ANY CLAIM, OR ANY SPECIAL INDIRECT OR CONSEQUENTIAL DAMAGES,
OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS,
WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION,
ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THE DATA
FILES OR SOFTWARE.

Except as contained in this notice, the name of a copyright holder shall
not be used in advertising or otherwise to promote the sale, use or other
dealings in these Data Files or Software without prior written
authorization of the copyright holder.
````

- `unicode-ident` ((MIT OR Apache-2.0) AND Unicode-3.0): https://github.com/dtolnay/unicode-ident
- `unicode-normalization` ((MIT OR Apache-2.0) AND Unicode-3.0): https://github.com/unicode-rs/unicode-normalization

## zlib License (`Zlib`)

````text
zlib License

This software is provided 'as-is', without any express or implied warranty.  In no event will the authors be held liable for any damages arising from the use of this software.

Permission is granted to anyone to use this software for any purpose, including commercial applications, and to alter it and redistribute it freely, subject to the following restrictions:

     1. The origin of this software must not be misrepresented; you must not claim that you wrote the original software. If you use this software in a product, an acknowledgment in the product documentation would be appreciated but is not required.

     2. Altered source versions must be plainly marked as such, and must not be misrepresented as being the original software.

     3. This notice may not be removed or altered from any source distribution.
````

- `crc-turbo` ((MIT OR Apache-2.0) AND Zlib): https://github.com/scryer-media/crc-turbo; ships no file with this licence's text
- `foldhash` (Zlib): https://github.com/orlp/foldhash
  - Copyright (c) 2024 Orson Peters
- `slotmap` (Zlib): https://github.com/orlp/slotmap
  - Copyright (c) 2021 Orson Peters <orsonpeters@gmail.com>

## bzip2 and libbzip2 License v1.0.6 (`bzip2-1.0.6`)

````text
--------------------------------------------------------------------------

The original program, "bzip2", the associated library "libbzip2", and all
documentation, are

Copyright (C) 1996-2021 Julian R Seward.
Copyright (C) 2019-2020 Federico Mena Quintero
Copyright (C) 2021 Micah Snyder

This Rust translation, "libbzip2-rs" is a derived work based on "bzip2" and
"libbzip2", and is Copyright (C) 2024-2025 Trifecta Tech Foundation and contributors

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions
are met:

1. Redistributions of source code must retain the above copyright
   notice, this list of conditions and the following disclaimer.

2. The origin of this software must not be misrepresented; you must
   not claim that you wrote the original software.  If you use this
   software in a product, an acknowledgment in the product
   documentation would be appreciated but is not required.

3. Altered source versions must be plainly marked as such, and must
   not be misrepresented as being the original software.

4. The name of the author may not be used to endorse or promote
   products derived from this software without specific prior written
   permission.

THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS
OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY
DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE
GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

Julian Seward, jseward@acm.org
bzip2/libbzip2 version 1.1.0 of 6 September 2010

--------------------------------------------------------------------------
````

- `libbz2-rs-sys` (bzip2-1.0.6): https://github.com/trifectatechfoundation/libbzip2-rs
