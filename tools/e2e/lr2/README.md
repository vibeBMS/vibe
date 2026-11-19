# e2e: LR2 on a hosted Windows runner

Phase 2 of the e2e plan: Lunatic Rave 2 (100201) run as a black box on `windows-latest`, to play the
`tools/lr2-measure` charts unattended and compare its result counts with vibeBMS's rows
(`crates/vibebms-core/tests/lr2_measure.rs`). The workflow is `.github/workflows/e2e-lr2.yml`.

LR2 is only run, and only its screen and the files it writes are read. LR2 comes from its official site, once,
into the Actions cache keyed by its SHA-256. No LR2 file is committed or uploaded as an artifact: the `e2e-lr2`
artifact holds only this workflow's own logs, screenshots and tables.

## Status: stopped at the download

LR2's official download was `http://www.dream-pro.info/~lavalse/LR2_100201.zip`, on LR2IR's host. LR2IR closed on
2026-05-31, and the host no longer resolves: from the runner, `DNS name does not exist`. LR2's first site,
`lr2.sakura.ne.jp`, is now an unofficial page ("no direct relationship with the developer, lavalse") that links
the Internet Archive's capture of that URL. That capture is the only copy of the official file left. It answers
from the runner (HEAD: 200, `application/zip`, 54,143,938 bytes), but it is a third party's copy, so the
workflow does not download it and no LR2 step is built.

To go on, the owner decides on the source. If the capture is accepted, set `LR2_URL` to
`https://web.archive.org/web/20190802100906id_/http://www.dream-pro.info/~lavalse/LR2_100201.zip`, run once,
and pin the SHA-256 that `fetch.ps1` prints in `LR2_SHA256`. From then on the zip comes from the cache.

One more thing: CI's `cache-prune` job deletes every cache entry made on a ref other than main whenever main is
pushed. An `lr2-<sha256>` entry saved by a run on `e2e/lr2` would therefore be gone after the next push to main.
Seed the cache from a run on main (workflow_dispatch), or have `cache-prune` keep keys starting with `lr2-`.

## What the runner gives LR2 (measured without LR2)

`lr2probe.exe` (`probe.c`, 32-bit as LR2 is, built with the runner's MSVC) uses the same Windows APIs as a
DirectX 9 game. `probe.ps1` runs it one stage at a time. Results from runs 44589382253 to 44594213681 (the last: job 135808029911); the
runs before it:

| question | answer on `windows-latest` (Windows Server 2025, image windows-2025-vs2026) |
|---|---|
| desktop | interactive session 2 (`runneradmin`), 1024x768, Microsoft Hyper-V Video adapter |
| Direct3D 9 | `Direct3DCreate9` works. One adapter, **Microsoft Basic Render Driver** (`d3d10warp.dll`, WARP). The HAL device takes hardware vertex processing, VS/PS 3.0, textures up to 16384. Windowed and fullscreen 640x480 both create and present, at about 3000 to 4500 fps with vsync off. |
| screen capture | ffmpeg's gdigrab captures a **windowed** D3D9 window (`d3d9-windowed.png`: the clear colour and the triangle). Exclusive **fullscreen** captures black: LR2 must run windowed. |
| audio, as the runner comes | no device: `waveOutGetNumDevs` 0, no active render endpoint, and `DirectSoundCreate8` fails with `0x88780078` (DSERR_NODRIVER). The Windows Audio service is running. |
| audio, with a virtual card | LABSN/sound-ci-helpers (VB-CABLE, pinned by commit) installs in about 4 s. Then there is one endpoint, DirectSound creates its primary and secondary buffers, and the play cursor advances in real time (481 ms in 500 ms). |
| locale | en-US, ANSI code page 1252, OEM 437. Shift-JIS text reads as mojibake. The Japanese fonts are already there (`msgothic.ttc` "ＭＳ ゴシック", Yu Gothic). Adding them as a Feature on Demand hung for 8 minutes and is not needed. |
| Locale Emulator | works: the probe under it sees code page 932 and LCID 0x0411, and Shift-JIS reads correctly. The 2.5.0.1 zip lacks `LECommonLibrary.dll`, which only its installer window writes; `probe.ps1` writes it out from the installer's resources, without the window. `LEProc.exe PATH ARGS` runs PATH with the default ja-JP profile. |
| SendInput | 200 presses 100 ms apart (`send`, busy-wait timing with `timeBeginPeriod(1)`) into a window reading them by message and by DirectInput 8 (buffered, foreground, non-exclusive). All 200 arrived both ways. Delay after the due time, DirectInput: mean 0.5 to 0.6 ms, sd 0.1 to 0.5 ms, p99 0.8 to 1.4 ms, worst single press 1.1 to 7.4 ms across the runs. The same holds when the window draws with D3D9 and polls once a frame. `SetForegroundWindow` works from the runner's processes. |

## What LR2's own documentation says (no runner evidence: no LR2)

- **Starting a chart:** BMSE's help, on using LR2 as an editor's viewer, gives `LR2body.exe -A -NS <file>` for
  autoplay from the start and `LR2body.exe -NS <file>` for a test play. Chart-only mode shows the result screen,
  then exits. With autoplay, or a score of zero, it exits with no result screen. `-NS` keeps the score out of the
  database. The chart-only mode wants a non-empty song database (JUKEBOX), set up once from LR2's select screen
  by dropping a folder onto it. The BMSE help also lists `-auto2avi <chart> <out.avi>`, which writes a video of an
  autoplay.
- **Locale:** the same help says that Windows in other languages needs the Japanese fonts (the runner has them),
  and that the system locale should be Japanese if LR2's text is garbled. Locale Emulator covers that, and
  ASCII-only charts (the lr2-measure ones are) do not need it.
- **Results:** LR2 keeps scores in SQLite, `LR2files\Database\Score\<player>.db`, in a table `score` keyed by
  `hash` (the chart's MD5), with `clear` the lamp. The judgement count columns should be read from the schema of
  the file LR2 writes on the runner. A play with `-NS`, or an autoplay, saves nothing, so counts read from the
  database need a scripted play without `-NS`.
- **Settings:** LR2 keeps them in `LR2files\Config\config.xml` and rewrites it when it quits. Its key names (vsync
  off, windowed) should be read from the file LR2 writes on its first run, then set by script before each play.

## The LR2 steps, once there is a zip

1. Unzip into `$RUNNER_TEMP` (never into `$OUT`). Install VB-CABLE first.
2. First run: start `LR2body.exe` and let it write `config.xml`. Log its element names, not the file. Set
   windowed and vsync off, then register the charts folder as JUKEBOX: post a `WM_DROPFILES` message to LR2's
   window, then wait for the song database.
3. Autoplay: run `LR2body.exe -A -NS a7-n200.bme` (under `LEProc.exe` if the text is garbled). Take gdigrab
   screenshots of the play screen.
4. Scripted trials: run `LR2body.exe <chart>` without `-NS`, and have `lr2probe send` press keys on a schedule.
   Read the counts from `score` by the chart's MD5 and compare with `lr2_measure.rs`'s rows, within the README's
   tolerances ("12 of 16 or more decides").

The hard part is step 4's clock. Nothing outside LR2 shows when its chart time starts. SendInput is precise to
about 1 ms, but the presses have to be placed against the chart's time. One way: give a copy of each chart (made
by script, not committed changes to `lr2-measure`) a BGM click at a known measure. Capture it from VB-CABLE's
output with WASAPI, and schedule the presses from the moment the click arrives. Calibrate the constant audio
delay once with a sweep chart: presses stepped 2 ms apart across `#RANK 0`'s windows, and the centre of the
PGREAT run is the offset.
