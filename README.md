<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/logo/logo-dark.svg">
    <img alt="vibeBMS" src="assets/logo/logo-light.svg" height="96">
  </picture>
</p>

A cross-platform BMS player modelled on [Lunatic Rave 2](http://www.lr2.sakura.ne.jp/) and
[beatoraja](https://github.com/exch-bms2/beatoraja) and its variants
([LR2oraja](https://github.com/wcko87/lr2oraja),
[LR2oraja Endless Dream](https://github.com/seraxis/lr2oraja-endlessdream)), with ideas from
[bmz-player](https://github.com/hyrorre/bmz-player).
Written in Rust with wgpu and cpal, it runs on Windows, macOS and Linux (x64/ARM) and in web browsers (wasm),
and has an on-screen controller for touch devices such as iPads and Android tablets.

The *vibe* in the name is the vibe of the music, and **vibe coding**: nearly all of the code and the default skins
were written with an AI coding agent (Claude Code), while a person set the direction and checked the results by
playing.

- No launcher: it starts at song select, and every setting is changed in the game.
- Settings, scores and caches live in a folder next to the executable (portable).
- BMS folders inside the executable's folder are scanned automatically; other folders can be added.

> Status: early development (0.1).

## Get it

Builds of the latest commit are on the [Nightly](https://github.com/vibeBMS/vibe/releases/tag/nightly) prerelease:

- **Windows, macOS, Linux**: the executables. The macOS app is `vibeBMS-macos-universal.dmg`.
- **Linux, older systems**: `vibebms-linux-*-portable` starts with glibc 2.28 and without WebKitGTK, but has no
  in-app browser.
- **Flatpak**: `flatpak install --user vibeBMS-linux-x64.flatpak` (or `-arm64`); a newer bundle installs over it.
- **Android**: `vibeBMS.apk`, also as an F-Droid repository for updates (its address is in the release's
  [`fdroid.txt`](https://github.com/vibeBMS/vibe/releases/download/nightly/fdroid.txt)).
- **iOS / iPadOS**: the unsigned `vibeBMS-unsigned.ipa`, to sideload; the release also has an AltStore / SideStore
  source.
- **Web**: [vibebms.github.io/vibe](https://vibebms.github.io/vibe/), or the release's web build to serve yourself.

The desktop executables look for a newer nightly on start and update in place from `SETTINGS › SYSTEM`. Building for
each platform, and checking that a release build is reproducible: [docs/BUILDING.md](docs/BUILDING.md).

## Run it

Put the executable in a folder and put your BMS folders inside it. Folders elsewhere can be dropped on the window or
added in `SETTINGS › LIBRARY`. Or build and run it:

```sh
cargo run --release                                  # song select
cargo run --release -- path/to/song.bme --autoplay   # play a chart directly
```

The data folder (`config/`, `scores/`, `cache/`, `skins/`) is next to the executable; `VIBEBMS_ROOT` moves it. The
command line, the data folder on each platform and every setting: [docs/MANUAL.md](docs/MANUAL.md).

## Controls

| | Keyboard defaults (as beatoraja) |
|---|---|
| 1P 7 keys | `Z S X D C F V`, scratch `LShift` / `LCtrl` |
| 2P 7 keys | `M K , L . ; /`, scratch `RShift` / `RCtrl` |
| 9 keys (PMS) | `A S D F G H J K L` |
| 4 / 6 / 8 keys | `D F J K` / `S D F J K L` / `A S D F J K L ;` |

| Where | Keys |
|---|---|
| Song select | ↑↓ move; Enter or a white key decide; ← or a blue key close the folder; Esc back; `/` search |
| Song select | F2, F4–F9 options; P watch a replay; E practice; F12 rescan; 1 / 2 / 4 key mode, sort, difficulty |
| Song select, holding START (`Q`) / SELECT (`W`) | beatoraja's play option panels |
| Play | ↑↓ hi-speed, PgUp/PgDn SUDDEN+, ←→ judge offset, Esc quit; START / SELECT for the in-play panel |
| Results | R retry, P watch this play again, 1–4 keep its replay in that slot |
| Anywhere | F1 / Tab settings, F11 full screen, Print Screen screenshot |

Controllers, IIDX controllers included, are bound in `SETTINGS › CONTROLS`. Touch screens get an on-screen
controller, with layouts of your own. Every key: [docs/MANUAL.md](docs/MANUAL.md#controls).

## Features

**Charts**
- BMS, BME, BML, PMS and BMSON in 5, 7, 10, 14 and 9 keys, and Qwilight-style 4, 6 and 8 keys.
- Shift_JIS, EUC-KR and UTF-8; `#RANDOM`, `#SCROLL`, `#SPEED`, negative BPM, long notes and mines.
- Songs in zip, 7z, RAR, LZH and tar archives are played without unpacking them yourself.

**Rules** ([docs/RULES.md](docs/RULES.md))
- beatoraja, LR2 and IIDX judge presets and gauge sets, each with its own records; custom windows.
- LN, CN and HCN, LN MODE, back-spin scratch, empty POOR (空POOR).
- Lane options from MIRROR to S-RANDOM+, F-RANDOM and TRAINER; DP FLIP; BATTLE.
- beatoraja's assists, chart modifiers (SCROLL REMOVE, MINE ADD, LONGNOTE ADD), LR2's FREQ, KEY CONVERT.

**Play**
- Hi-speed types: classic, HS-FIX, IIDX's NORMAL and FLOATING, LR2's; SUDDEN+, HIDDEN+, LIFT.
- Image and video BGA; animated images in charts and skins.
- Replays of every play, watched at any speed, and ghosts; a practice screen for a section of measures.
- G-BATTLE against your own replay; VS for two players on one machine.
- Dan and courses, LR2's course kinds (class, expert, NONSTOP) included; target scores and rivals.
- Clear lamps, EX score, FAST / SLOW, gauge and timing graphs.

**Library**
- Difficulty tables: a chart not installed is downloaded with its song in the app, or a whole level at once.
- Separate charts (差分) go into their song's folder.
- The BOF events of [vibeBMS/BMS](https://github.com/vibeBMS/BMS) as folders, downloaded from the event mirror.
- Score import from LR2 and beatoraja; profiles for several players on one library.

**Third-party sources** are off by default. They are other people's copies of songs whose links are dead; vibeBMS
says what they are and asks once before they are on. Using them is your own responsibility: see BMSeeker's
[Third-party sources and disclaimer](https://github.com/vibeBMS/BMSeeker#third-party-sources-and-disclaimer).

**Online**
- IR rankings and score submission over vibeBMS's JSON protocol ([docs/IR_PROTOCOL.md](docs/IR_PROTOCOL.md)), and
  adapters for Bokutachi and BMS-IR.
- Your scores on an IR downloaded into your local records.

**Audio and display**
- Low-latency output with keysounds scheduled to the sample; WASAPI exclusive on Windows; ASIO and JACK as build
  features.
- Loudness normalisation, so every chart plays at the same loudness.
- Present modes from smooth vsync to unlimited; the graphics API can be chosen.

**Other**
- English, Korean, Japanese, Simplified and Traditional Chinese.
- Discord Rich Presence and OBS scene switching and recording.
- Screenshots, video export of a chart's autoplay or replay, and a chart viewer for BMS editors.

## Skins

Play, song select and results are drawn by skins, chosen in `SETTINGS › SKINS`; a change applies without a restart.

- Built-in skins with landscape and portrait layouts for every mode, and a touch perspective play skin whose lanes
  are the controls.
- LR2 skins (`.lr2skin`), and beatoraja JSON and Lua skins, put under `skins/`.
- `GET SKINS` downloads skins from a catalog. vibeBMS bundles no third-party skins; skins whose terms do not allow
  redistribution need third-party sources on.
- Sound sets: beatoraja's, LR2's, and bmz-player's conventions.

What is supported, and vibeBMS's skin extensions: [docs/SKINS.md](docs/SKINS.md).

## Web version

The web build runs in the browser: [vibebms.github.io/vibe](https://vibebms.github.io/vibe/). Pick a folder, files or
song packs; they are kept in the browser, and nothing is uploaded anywhere. Add the page to the home screen to run it
full screen. How it differs from the desktop: [docs/MANUAL.md](docs/MANUAL.md#web-build); building it:
[docs/BUILDING.md](docs/BUILDING.md#web-wasm32).

## Documentation

- [docs/MANUAL.md](docs/MANUAL.md): controls, settings, the library and downloads, replays, IR, the web build.
- [docs/SKINS.md](docs/SKINS.md): skin support and vibeBMS's skin extensions.
- [docs/RULES.md](docs/RULES.md): judging, gauges, options and what a play records, with their sources.
- [docs/LR2_BEHAVIOUR.md](docs/LR2_BEHAVIOUR.md), [docs/LR2_COURSES.md](docs/LR2_COURSES.md): the LR2 behaviour
  specifications RULES.md follows.
- [docs/IR_PROTOCOL.md](docs/IR_PROTOCOL.md): the JSON protocol for IR servers.
- [docs/BUILDING.md](docs/BUILDING.md): building, packaging and releases.

## Layout

```text
crates/vibebms-core     parsing, timing, judging, gauges, scores (no platform dependencies)
crates/vibebms-audio    decoders and mixer
crates/vibebms-ir       IR client
crates/vibebms          the app (winit + wgpu, skins, scenes)
crates/vibebms-web      web entry point
crates/vibebms-lua-web  Lua for the web build
crates/vibebms-android  Android entry point
```

Difficulty tables, song downloads, unpacking and placing separate charts come from
[BMSeeker](https://github.com/vibeBMS/BMSeeker) (a library and a CLI).

## License

`MIT OR Apache-2.0` (LICENSE-MIT, LICENSE-APACHE). The embedded Inter font is SIL OFL 1.1; the logo's letters are
Inter converted to outlines. Lua's pattern matcher (ported) and the BeMusicSeeker fork's rules are MIT. RAR files are
unpacked with unrar-rs, under the UnRAR licence restriction. These notices are in full in
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md); the licences of the Rust crates it is built from, generated from
Cargo.lock, in [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).

The judging, gauge and skin rules follow the documented behaviour of beatoraja, LR2oraja and LR2
([docs/RULES.md](docs/RULES.md)), but no code is taken from these players: the judge engine, gauges, chart metrics,
skin path matching and the SDF text shader were written from behaviour specifications and tests. No charts or music
are included.
