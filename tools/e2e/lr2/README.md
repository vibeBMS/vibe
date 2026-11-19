# e2e: LR2 on a hosted Windows runner

Phase 2 of the e2e plan: Lunatic Rave 2 (100201), run as a black box on `windows-latest`, plays the
`tools/lr2-measure` charts unattended. Its counts and gauge readings are compared with vibeBMS's rows
(`crates/vibebms-core/tests/lr2_measure.rs`, the lr2-measure README). The workflow is
`.github/workflows/e2e-lr2.yml`.

LR2 is only run. The workflow reads only LR2's screen and the files LR2 writes (`config.xml`, the score
database). It drives LR2 the way a person does: clicks and text in LR2's setup dialog, a folder picked in Browse
for Folder, and keys. Nothing is decompiled or read from LR2's memory. No LR2 file is committed or uploaded: the
`e2e-lr2` artifact holds this workflow's own logs, screenshots and tables.

## LR2

LR2's official download, `http://www.dream-pro.info/~lavalse/LR2_100201.zip` (LR2IR's host), has been gone
since LR2IR closed on 2026-05-31. The owner approved the Internet Archive's capture of that URL:
`https://web.archive.org/web/20190802100906id_/http://www.dream-pro.info/~lavalse/LR2_100201.zip`. The zip is
54,143,938 bytes, SHA-256 `34294ea6661b232071688a93de31631e4e8114a82314d3386541e6875a74ff41`. The workflow
pins that hash and keeps the zip in the Actions cache under `e2e-lr2-<sha256>`. CI's cache prune keeps `e2e-*`
keys on every ref.

## What a run does

| step | how |
|---|---|
| tools | `lr2probe.exe` (`probe.c`, 32-bit, built with the runner's MSVC); ffmpeg; VB-CABLE (LABSN/sound-ci-helpers), since the runner has no sound device and LR2's DirectSound needs one; Locale Emulator (`le.ps1`), since LR2's text is Shift-JIS and the runner's code page is 1252. |
| setup | `LR2.exe`'s setup dialog, driven through its controls (`lr2probe ui`). It creates a new player `e2etest`, selects it, turns on window mode (gdigrab cannot capture exclusive fullscreen), turns vsync off, and sets the sound buffer to 128. The song folder, `Desktop\lr2songs`, is added through JUKEBOX1's Browse for Folder by typing its name: a drag and drop is refused. "LR2の起動" then starts LR2 once. LR2 writes `config.xml` (logged: element names, plus the four values set here) and reads the folder. Screenshots of the title and select screens. |
| autoplay | `LR2body.exe -A -NS a8-mines.bme`, with screenshots of the play screen. |
| start signal | `e2e.py gen` gives each chart a copy with a BGM click at measure 1 (chart time 1600 ms, before any note). `lr2probe play` hears it on VB-CABLE's capture side. From the click it places the chart's time on the performance counter and presses keys by SendInput scan codes (LR2 reads DirectInput) at the chart times `e2e.py plan` gives. Default keys: Z S X D C F V, scratch on left Shift and Ctrl. |
| latency | Our own click takes about 63 ms through the cable (`loop`). LR2's output latency is calibrated by sweeps (`cal-*.bme`, 85 to 91 notes, each pressed 1 to 2.5 ms later than the one before). One sweep crosses only the early edge of the GOOD window, the other only the late edge: the difference of their hit counts gives the offset, their sum the window. A coarse pair on EASY, then a fine pair on VERY HARD. |
| trials | `LR2body.exe <chart>` (scores saved), the keys pressed as the lr2-measure README says. Counts are read from `LR2files\Database\Score\e2etest.db` (table `score`, keyed by the chart's MD5: `perfect great good bad poor`, `clear`, `maxcombo`, ...). There is a screenshot of each result screen. |
| gauge | `lr2probe` takes screenshots of LR2's window at chart times: five 40 ms apart for each reading. `e2e.py gauge` reads the gauge bar's highest lit segment (2 % each). It finds the bar from a GROOVE screenshot before the first note (20 %). The HARD gauge is `config.xml`'s `play/gauge` = 1; every A4 play's start reading of 100 % confirms it. |
| compare | `e2e.py compare`: the README's tolerances for counts ("12 of 16 or more decides"), exact for A8's counts and the gauge. |

## Results (runs 44607740845 and 44624982986)

All of vibeBMS's rows that LR2 was asked for came out as vibeBMS predicts, except two A7 readings that are one
segment up (below):

| | LR2 | vibeBMS |
|---|---|---|
| A11 `a11-1` BAD / POOR | 16 / 0 (PGREAT 32: the chain takes the second key-1 note) | 16 / 0 |
| A11 `a11-2` GOOD / POOR | 16 / 16 | 16 / 16 |
| A11 `a11-3` BAD / POOR | 32 / 0 | 32 / 0 |
| A3, 70 ms late, `#RANK` 0 2 3 4 5 -1 | BAD, GOOD, GOOD, GOOD, GOOD, GOOD (16 each) | 4, 5, -1 as 2 |
| A3, 110 ms late, the same | BAD, BAD, GOOD, BAD, BAD, BAD | 4, 5, -1 as 2 |
| A4, HARD after 3 empty POORs, at 20 29 30 31 45 60 90 110 124 125 notes | 40 50 58 58 64 70 72 74 74 76 | the same: **A4 closes on vibeBMS's row** (50 at 29 notes, not 56) |
| A4 `a4-t200`, after each POOR | 84 70 56 42 | the same: TOTAL 200's factor is 10/7, not S1's 1.5 |
| A8 run 1 (GROOVE, key 1 held), after each mine | 96 76 26 2, POOR 4, max combo 8, `clear` 5 | 96 76 26 2, POOR 4, FULL COMBO |
| A7 `a7-n200` | 30 40 48 58 68 78 88 96 100 (second run: 98 for the 96) | 28 38 48 58 68 76 86 96 100 |
| A7 `a7-n500` | 24 30 34 40 46 50 56 60 66 70 76 80 86 92 96 100 (both runs) | 24 30 34 40 44 50 54 60 66 70 76 80 86 90 96 100 |
| A7 `a7-n1000` | 22 26 30 34 36 40 44 48 52 54 58 62 66 68 72 76 80 82 86 90 94 96 100 | 22 26 30 34 36 40 44 48 50 54 58 62 64 68 72 76 78 82 86 90 92 96 100 |

A7: every LR2 reading is vibeBMS's or one segment (2 %) above it. Almost every "above" comes where vibeBMS's
exact gauge is well into the next segment (29.6 % read as 30, 45.6 as 46, 51.68 as 52), and one came at 0.4 of a
segment (96.8 read as 98, in one of two runs). LR2's bar lights the segment above the shown number at times. The
bar reading therefore cannot tell 28 from 30 there, but it rules out the README's TOTAL 160 rows by a wide margin.
Reading the gauge's digits instead of its bar would settle these readings. `a7-n1000` did not start in the first
run (LR2 never reached the click), so a play is now retried once.

Both runs passed the comparison step (`compare.md` in the artifact; the second run's job is 135916281981).

Other measurements, all from the score database:
- LR2's GOOD windows are ±40 ms on VERY HARD (fine sweeps: 39.5 to 41.5 with 1 ms steps) and about ±120 ms on
  EASY (coarse: 115 to 122.5 with 2.5 ms steps).
- About 17 to 18 PGREATs per ms-stepped sweep on VERY HARD, which suggests ±8 ms.
- `clear` codes seen: 1 on a failed GROOVE play, 3 on a cleared one, 5 on A8's FULL COMBO.

## Limits

- **The start signal moves from play to play.** Three calibrated VERY HARD sweeps (`cal-f3` .. `f5`) were 9.5,
  3.5 and 11.5 ms early in one run, and 0.5, 5.5 and 3.5 ms late in the next. The likely cause is that LR2 starts a sound on its next mix buffer (cut from 384 to 128
  samples for this; not measured apart). That is fine for these trials, whose margins are tens of ms. A comparison that needs 1 ms would
  need a sweep inside each play.
- **Not covered:** A8 runs 2 and 3 (tapping the mines, and HARD held), since one chart keeps one score row. They
  would need copies of the chart.
- **Run time:** about 42 minutes (job 135916281981). Each play costs about 9 s of loading plus the chart. A11, A3 and A8 alone take
  about 20 minutes. The runs could be split over parallel jobs, each with its own setup and calibration (about 6
  minutes).
- **JUKEBOX in config.xml:** `config.xml` has a `jukebox/path` element, so the folder could also be set there.
  The dialog route is used because `config.xml` exists only after the dialog has written it.

## Files

`lr2.ps1` (stages), `e2e.py` (charts, schedules, calibration, scores, gauge, comparison), `probe.c` / `build.cmd`
(`lr2probe`: window and dialog driving, drag and drop, the audio start signal, key schedules, screenshots;
also the runner probes), `le.ps1` (Locale Emulator), `fetch.ps1` (download and hash), `probe.ps1` and
`jitter.py` (the runner probes without LR2: D3D9, sound, code page, SendInput timing; workflow_dispatch input
`probe`).
