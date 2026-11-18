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

The *vibe* in the name means two things: the vibe of the music in a rhythm game, and that this project was made
by **vibe coding**. Nearly all of the code and the default skins were written in conversation with an AI coding
agent (Claude Code); a person set the direction and checked the results by playing.

- No launcher: it starts at song select, and every setting is changed in the game.
- Settings, scores and caches live in a folder next to the executable (portable).
- BMS folders inside the executable's folder are scanned automatically; other folders can be added.

> Status: early development (0.1). Tested on the desktop (Linux) and in browsers; the Android and iOS builds have
> not been tried on real devices yet.

## Quick start

Builds of the latest commit, for every platform, are on the
[Nightly](https://github.com/vibeBMS/vibe/releases/tag/nightly) prerelease: the executables, the Android `.apk` and
the unsigned iOS `.ipa` as they are, the web build as a zip. On Linux, `vibebms-linux-*-portable` starts on older
systems (glibc 2.28) and without WebKitGTK, but has no download browser. `vibeBMS-linux-x64.flatpak` / `-arm64.flatpak` are Flatpak
bundles: `flatpak install --user vibeBMS-linux-x64.flatpak` (the runtime comes from Flathub), and the same with a newer
bundle to update it; its data is in `~/.var/app/io.github.vibebms.nightly/data/vibeBMS`.

The desktop executables from there look for a newer nightly on start (`SETTINGS › SYSTEM › Check for updates`).
When there is one, the song select says so and `SETTINGS › SYSTEM` updates in place ("Update now", then "Restart
now"); a copy a package manager installed (or the Flatpak), or one in a folder that cannot be written to, does not
check.

Or build it yourself:

```sh
cargo run --release                                  # song select
cargo run --release -- path/to/song.bme --autoplay   # play a chart directly
```

Put a release build of the executable in a folder and put your BMS folders inside it. Folders elsewhere can be
dropped on the window or added in `SETTINGS › LIBRARY`. Building for each platform: [docs/BUILDING.md](docs/BUILDING.md).

```text
<root>/config/config.toml      settings (edited in the game; editing by hand works too)
<root>/scores/scores.json      play records
<root>/scores/player.json      totals over all plays (judgements, time played)
<root>/cache/                  song scan, archives' file lists and difficulty table caches (made again if deleted)
<root>/skins/                  LR2 / beatoraja skins
<root>/sounds/                 system sound sets (optional)
<root>/fonts/*.ttf|otf|ttc     fonts to use (optional)
```

If the executable's folder cannot be written to, the user data folder is used; `VIBEBMS_ROOT` sets the location.

## Controls

| | Keyboard defaults (as beatoraja) |
|---|---|
| 1P 7 keys | `Z S X D C F V`, scratch `LShift` / `LCtrl` |
| 2P 7 keys | `M K , L . ; /`, scratch `RShift` / `RCtrl` |
| 9 keys (PMS) | `A S D F G H J K L` |
| Anywhere | F11 full screen; Print Screen (F13 on Apple keyboards; song select's SCREENSHOT button) saves the frame as `screenshots/YYYYMMDD-HHMMSS_<title>_<difficulty>_<lamp>_<EX>.png` in the data folder (Android and iOS: the app's storage, no share sheet yet; web: a download). The frame is copied on the GPU and read back a few frames later, so play does not stall |
| Song select | ↑↓ move; Enter, → or a white key: decide (open a folder, play a song); ← or a blue key: close the folder; Backspace / Esc back; `/` search; F2–F9 options; F12 rescan; P watch the chart's replay; E practice the chart (PRACTICE, the last bottom button); 6 key config, 7 skin select; 1 key mode filter (ALL / 7 / 14 / 9 / 5 / 10 KEYS), 2 sort (title, level, clear, score rate, miss count, last played, newest, BPM, artist, notes), 4 difficulty filter (ALL / BEGINNER … INSANE): kept in `[select]`, also on the OPTIONS buttons and skin buttons 10 / 11 / 12; 8 / 9 favourite song (every chart in its folder) / chart, listed in the FAVORITE folder at the root and marked ♥ (`scores/favorites.json`; on touch, the OPTIONS buttons FAV CHART / FAV SONG); after it, virtual folders: LAMP UPDATE and SCORE UPDATE (charts whose lamp a play raised to a clear, or whose best EX score a play raised, in the last 7 days; records from before this was kept are in neither until played again), CLEAR TYPE (a folder per lamp), SCORE RANK (per DJ level, NO PLAY last), LEVEL (per level) and NEW (charts whose file came or changed in the last 7 days, the time the newest sort goes by), all with the filters and by the records of the judge preset chosen; RANDOM SELECT, the second bar of each folder with charts, plays one of them drawn at random (as filtered), 0 (or its OPTIONS button) choosing from which: ANY, NOT CLEAR (below CLEAR), NOT HARD (below HARD), NOT FC, by the lamps of the judge preset chosen |
| Play | ↑↓ hi-speed, PgUp/PgDn SUDDEN+, ←→ judge offset, Esc quit |
| Song select, holding START (`Q`) / SELECT (`W`) | play option panels (beatoraja's key layout). START: 1▼2▲ RANDOM, 3 GAUGE, 4 DP OPTION, 5 HS-FIX, 6▲7▼ 2P RANDOM, turntable TARGET. SELECT: 1 EXPAND JUDGE, 2 CONSTANT, 3 JUDGE AREA, 4 LEGACY NOTE, 5 MARK NOTE, 6 BPM GUIDE, 7 NO MINE. Both (or number key 5): 1 BGA, 2 GAUGE AUTO SHIFT, 3 judge timing auto-adjust, 4·6 green number −/+ (repeats while held), 5·7 judge timing −/+. Number key 3: LN mode |
| Play, START (`Q`) / SELECT (`W`) | beatoraja's in-play panel: START + white key hi-speed −0.25, blue key +0.25; START + turntable lane cover (LIFT without a cover); START twice toggles the cover; SELECT + keys / turntable hi-speed ±0.01; START + SELECT for a second quits |
| Results | R / F5 retry, P watch this play again |
| Anywhere | F1 / Tab settings, F11 full screen |

Entering a folder puts the cursor on the folder's own row; deciding on it closes the folder (as in LR2).

- beatoraja's assist options: EXPAND JUDGE (wider windows; PG/GR/GD ratios in the settings), JUDGE AREA (the judge
  windows drawn above the judge line), MARK NOTE (notes hit early stay grey down to the judge line), BPM GUIDE (BPM
  changes and stops shown), judge timing auto-adjust. As in beatoraja, EXPAND JUDGE over 100% makes the play
  ASSIST EASY and BPM GUIDE on a chart whose BPM changes LIGHT ASSIST EASY; such plays keep only their lamp and
  play count (see Clear lamps in `docs/RULES.md`).
- Hi-speed (`SETTINGS › GAMEPLAY › Hi-speed type`):
  - CLASSIC (default): you set the multiplier (in steps of 0.25); the green number (how long notes are visible)
    follows from it, the BPM and the lane cover. HS-FIX (START BPM / MAX BPM / MAIN BPM / MIN BPM) does the
    opposite: the green number stays and the multiplier follows each chart's BPM.
  - NORMAL: IIDX's normal hi-speed, levels 1 to 20 that are green numbers of the whole lane (1200, 1000, 800,
    700, 650 … 300, 280, 260, as bmz-player documents IIDX's), at the HS-FIX BPM (MAIN without HS-FIX); the hi-speed
    keys step a level.
  - FLOATING: IIDX's floating hi-speed. The green number is kept: through the lane cover and LIFT, and while
    playing through the chart's BPM changes (the multiplier follows the BPM of the moment; before the start, the
    HS-FIX BPM). The hi-speed keys set a new green number.
  - LR2: the multiplier, as LR2 has it; with HS-FIX it is the speed at 150 BPM (the notes of a chart's HS-FIX BPM
    move as a 150 BPM chart's at that setting), the setting is what the screen and skins show, and the lane cover
    leaves it as it is.

  A change made while playing reaches the notes over 150 ms, easing out (`Hi-speed change while playing`, 0 to
  1000 ms; 0: at once), and the new hi-speed and green number show over the note field for a moment. A change the
  chart makes (FLOATING through a BPM change) is not eased.
- Controllers (IIDX controllers included) are set in `SETTINGS › CONTROLS` by pressing their buttons. Analog
  turntables work; their dead band (sensitivity) and stop time are set there, with a key chatter filter (off by
  default; see [docs/RULES.md](docs/RULES.md) "Input filters").
- Touch (`SETTINGS › CONTROLS`): a BM layout (turntable and two rows of keys; round pop'n buttons for PMS), a single row
  of keys, or your own: the layout editor (`KEY CONFIG › Touch controller layout`) moves and resizes the buttons (drag
  them, drag their corners or pinch them) for each key mode and orientation, flips a layout left to right (the
  turntable changes sides; custom layouts are otherwise used as placed, whatever the play side), and sets the height of
  the controls (drag their top edge). Portrait or landscape SP and landscape DP, multi-touch and sliding. A touch
  between keys plays the nearest one. Keys are sized like an arcade's (the screen size is guessed from the device and
  can be set), and the START / SELECT buttons in the letterbox beside the play screen (on the turntable's side: a
  custom layout's turntable, else left for 1P and DP, right for 2P; or the side chosen) open the panels above. A key whose release the browser never delivers
  (an iPad's home bar dragged with a key held) is let go once no finger is on the screen.

## Features

- **Charts**: BMS/BME/BML/PMS and BMSON. `#RANDOM`/`#SWITCH` control flow, BPM changes, STOP and `#SCROLL`, LN
  (`#LNTYPE`/`#LNOBJ`/`#LNMODE`), mines, 5/7/10/14/9 keys (`.pms` in the standard or the BME-type layout); Shift_JIS, EUC-KR and UTF-8 detected.
  The scan reads new and changed charts several at a time (up to 8 threads natively; one at a time on the web).
- **Songs in archives**: a `.zip`, `.7z`, `.rar` or `.lzh` (`.lha`) in the library is played without unpacking it
  yourself. It is scanned as a folder (its charts in sub-folders too) and its charts are named by the archive, a `!`
  and their path in it: `bms/packs/pack.zip!/Song/chart.bme` (song select lists an archive right in a library
  folder as a folder of its own, `pack.zip`; that path also plays from the command line). An archive is read
  once when it is new or its size or time changed: its list of files goes into `cache/archives.json`, and only its
  charts are read (a zip's charts alone; a 7z, RAR or LZH, solid ones included, is decoded from the start once, its
  other files not kept). An unchanged archive is not opened by the scan. Playing a chart from one unpacks its song,
  the folder that holds the chart in the archive with its sub-folders, once: into memory, or when it is larger than
  512 MB (192 MB on Android and iOS) into `cache/unpacked/` (emptied at start). It is kept while the song stays
  chosen (back from the result screen, playing it again unpacks nothing) and let go when another is chosen. The
  song preview, the stage file and banner, the chart's text files and its videos (written out of the archive for
  ffmpeg) work as for a folder; outside a play a zip's files are read where they are, a 7z's, RAR's or LZH's
  song is unpacked on its first file read. Nothing is written into an archive: song downloads take the songs in
  archives for not installed (a separate chart for one is not put into it). A chart that is also a file of its own (the archive unpacked beside it) is
  listed once, as the file. Not read: archives in an archive, `.tar.gz`, encrypted archives.
- **Judging**: beatoraja / LR2 / IIDX presets and custom windows. Rules and sources: [docs/RULES.md](docs/RULES.md).
  Each preset keeps its own records (best EX score, lamp, min BP and min CB) and replays of a chart; song select shows
  those of the preset chosen.
  The note a press takes when several are in reach (`SETTINGS › GAMEPLAY › Judge algorithm`): COMBO, DURATION,
  LOWEST or SCORE (the next note only when the current one is beyond GREAT late and the next is within GREAT), as
  beatoraja's; a replay keeps the one it was played with.
- **Gauges**: ASSIST EASY to HAZARD, dan gauges, GAUGE AUTO SHIFT.
- **Notes**: LN / CN / HCN, back-spin scratch, mines, empty POOR (空POOR).
- **Options**: MIRROR, (R/S/H-)RANDOM, SPIRAL, ALL-SCR, RANDOM+, DP FLIP, BATTLE / BATTLE AS (with AUTO SCRATCH on
  both sides), assists, floating / fixed hi-speed,
  SUDDEN+ / HIDDEN+ / LIFT. Assisted plays keep only their lamp and play count and are not sent to IR; plays in
  a course leave no record for the song.
- **Display** (`SETTINGS › DISPLAY & AUDIO › Present mode`): SMOOTH (the default: vsync with two frames queued, each drawn
  for when it will be on the screen; even motion at the display's refresh rate, 120 Hz and more included), LOW
  LATENCY (the newest frame at each vblank without tearing, where the GPU has mailbox presentation; elsewhere, as on
  macOS and iOS, the same as SMOOTH) and UNLIMITED (as many frames as the GPU makes, with tearing). Android asks for
  the screen's highest refresh rate; the iOS app allows ProMotion iPhones above 60 Hz. `Show FPS` shows the rate
  reached.
- **Song preview** (`SETTINGS › DISPLAY & AUDIO › Song preview`: LOOP, the default; ONCE, then the select BGM again; OFF):
  the chart's `#PREVIEW`, else a `preview*` sound in its folder, else one made of the chart's sounds: its BGM and
  keysounds mixed over 15 s (10 s on the web and mobile) from its first dense part (the first stretch from a fifth
  of the chart on with 85 % of the notes of the densest, from its bar line). It is made in the background (natively
  off the main thread; on the web only once the cursor has rested 1.2 s, and only the sound files of that stretch are
  read), at the preview volume, and the last 8 made (3 on the web and mobile) are kept in memory.
- **BGA**: image BGA (layers, POOR; a BMS chart without a POOR layer shows its `#BMP00` on a miss, as LR2 and
  beatoraja do), video BGA (desktop, with an installed `ffmpeg`; `VIBEBMS_FFMPEG` sets its path). BGA ON, AUTO or
  OFF (SYSTEM OPTION key 1), as beatoraja's: AUTO shows it only when watching (autoplay, a replay), not in a play
  of your own.
- **Play extras** (`SETTINGS › GAMEPLAY › PLAY EXTRAS`):
  - FAST / SLOW threshold: the play skin's FAST and SLOW (options 1242 / 1243) show only for a judgement more
    than this many ms off (0, the default: any; FAST / SLOW display off: never).
  - GUIDE SE (off by default): a sound for each judgement as it is made, from the sound set chosen by beatoraja's
    names (`guide-pg`, `guide-gr`, `guide-gd`, `guide-bd`, `guide-pr` for an empty POOR, `guide-ms` for a missed
    note; any audio format), and for what the set has none of (an LR2 sound set, the built-in set) a built-in tone.
    At the system sound volume; G-BATTLE's opponent is silent.
  - Notes wait at the line (off by default): as LR2 draws them, a note (a long note's head) not yet judged stops
    at the judge line once its time has come, until it is hit or missed, instead of scrolling past (the flat and
    the perspective note fields; a skin's PMS falling notes as they are).
  - LR2oraja Endless Dream's extras, each off by default: RESULT WAIT (the play goes on to 5 s after the chart's
    last note, BGM or BGA layer, the POOR layer left out, instead of ending a second after the last judgement; not
    after a practice section, a quit or a fail; a long BGM sound is not followed to its end); GOOD clears the miss
    BGA (a PGREAT, GREAT or GOOD takes the POOR image away at once, the next BAD or POOR shows it again); keysound
    on a missed note (a note passed without a press plays its keysound as its POOR comes: a CN / HCN's head once for
    its two POORs, and its end sound when held past its end; not a note a press already sounded, an empty POOR, an
    early release or a mine). None changes how a play is judged or recorded.
- **Dan / courses**, clear lamps, EX score, FAST/SLOW, gauge and timing graphs, target score (personal best,
  rank A–AAA, MAX- / MAX, an IR rival). The IR rivals your enabled accounts follow come after BEST in the TARGET list
  (`RIVAL <name>`): the target is the EX score the IR has of theirs for the chart (0 when it has none), paced at the
  share of the notes judged, as the IRs give no per-note data; skins get the rival's name as text 1 and the target's
  name as text 3, its EX score as the target's numbers.
- **Difficulty tables**: bmstable format. The insane and normal tables, Satellite and Stella are there from the
  start. Charts a table lists that are not installed show dimmed; choosing one downloads its song and its separate
  chart (差分) in the app (a manbow work page is read for its download links; Google Drive, Dropbox, MediaFire, MEGA
  and plain archive servers work; the web build goes through the vibeBMS proxy). When the song is in the library
  already, only the chart is fetched, and a message says when none of its links works. What a download holds is
  sorted first, the same on the desktop and the web: a song new to the library gets a folder of its own; a song the
  library has already is not added again ("already installed"; only files its folder lacks come in); a separate
  chart goes into its song's folder with its own files as they are laid out (its `sound/` folder too), the song found
  by the library's charts in the same download, then by the song's own charts the table names (`org_md5`), then by
  the sounds it uses and its title and artist; when two folders are as likely, it waits rather than picking one.
  Links that cannot be downloaded directly (OneDrive, uploaders, deleted files) open their download page: on the
  desktop in a browser window in the app, whose downloaded zip/7z/rar/lzh is unpacked into `bms/` before the chart's page
  opens; a downloaded chart goes into its song's folder (the folder with the same title and artist, as the library
  was last read, that holds every sound it needs; when unsure, the app asks). Downloads are kept in `downloads/`. Android and iOS use the in-app
  browser too (over the game screen); the web build opens the system browser.
- **Third-party mirrors** (`SETTINGS › LIBRARY`, off by default): other people's copies of dead uploaders and sites
  (darksabun.club, bms.hexlataia.xyz) and the community archive (bms.kyouko.moe). What is found there by name only
  goes into the library if it holds the chart wanted (by its MD5).
- **Name song folders after their charts** (`SETTINGS › LIBRARY`, off by default): a song downloaded into the library
  gets a folder named `[artist] title` from what its charts share, as BeMusicSeeker names one (a folder its archive
  wraps everything in left out), instead of the song's title; a song the library has under another folder name (a
  chart of the same title and artist) goes into that folder.
- **Event mirror**: the BOF events of [vibeBMS/BMS](https://github.com/vibeBMS/BMS) (BOF22 to BOF2004) show as
  folders in song select. For events mirrored as they were (BOF22, BOF21, BOF:TT, BOF:XVII, BOF:XVI), a work not
  installed is downloaded from the mirror and unpacked into `bms/<event>/`; for the others the app downloads from
  the authors' links (Google Drive files and folders, Dropbox files and folders, MediaFire, MEGA, servers that send
  the archive). A link that needs a person on its page (OneDrive, uploaders), or that gives a page instead of an
  archive, opens that page. The web build works the same way: these hosts and GitHub's releases send no CORS
  headers, so it downloads through the vibeBMS proxy `proxy.vibebms.workers.dev`, which passes on archives only,
  and unpacks them in memory (zip, 7z, rar). Other event lists or indexes can be added in the settings
  (`archives`).
- **Replays and ghosts**: every play of your own (not a course stage, not autoplay) keeps a replay: what drove the
  judge (inputs and the times it was advanced to) with the seed and play options that made the chart, so watching
  it judges exactly the same. Each chart keeps its latest play's and its best EX score's, per LN mode and judge preset (`scores/replays/`; in the
  web build, in the browser). Watch one with P on the results (that play) or in song select (the chart's best,
  else its latest); a replay is not recorded. A BATTLE play's replay can be watched from its results but is not
  kept as the chart's latest or best. With TARGET MY BEST, the pacemaker is the best play's ghost: its EX
  score at the same note, not the best score spread evenly over the chart.
- **Practice**: E in song select (or the PRACTICE button) opens a chart's practice screen: its notes per
  measure as a graph, with a section of measures to play (tap the graph to move its nearer edge), a lead-in of
  0–4 measures before it (the music only), speed 50–200 % in 5 % steps (the sounds resampled, their pitch with
  the speed; the judge windows stay in real time, so a slower play is easier), the gauge and its start value,
  `#RANK` and TOTAL in place of the chart's, RANDOM, LN mode and LOOP (the section again at once when it ends or
  the gauge empties). The play starts at the lead-in with what was sounding there; a long note that starts
  before the section is not played (its sound is background), one that starts in it is played whole; the gauge
  moves by the whole chart's TOTAL and notes. In play R or Backspace starts the section again and Esc goes back
  to the practice screen, which shows the last try's judgements and timing. Practice plays are never recorded:
  no score, lamp, replay, play history or IR (settings in `[practice]`, the last section per chart).
- **G-BATTLE**: play a 5 or 7 key chart against your own MY BEST or LAST replay of it (song select's BATTLE row,
  F1, or DP OPTION in the START panel: OFF / BATTLE / BATTLE AS / G-BATTLE MY BEST / G-BATTLE LAST). The chart
  is made with the replay's seed and lane option on both sides (as LR2 lends a ghost's random); you play the
  first side, the replay's inputs play the second again under its own options (judge, gauge, LN mode, AUTO
  SCRATCH), silently. Its EX score is the pacemaker target as it plays; if it ever judges other than its
  replay did (a judge that changed since), the play shows GHOST DESYNC and the replay's EX score at your note
  stands in. Your side is saved as a normal play (lamp, scores, replays and IR as any other, the opponent's
  lane option counting for the assist level). Replays of BATTLE or LEGACY NOTE plays, of another LN mode, or
  without inputs cannot be opponents. On a phone held upright the single play skin is shown and the opponent
  is only the pacemaker. The results name the winner by EX score; RETRY plays the same battle again.
- **VS**: two players here, one per side of a 5 or 7 key chart (the BATTLE row's last value, or DP OPTION's).
  Each has their own keys (1P / 2P key config), START / SELECT (2P: `P` / `O` by default, `2P START` / `2P
  SELECT` in the key config), hi-speed, lane cover, LIFT, HIDDEN+, gauge, AUTO SCRATCH and judge offset
  (`[play.p2]` in the config; each player's START panel changes their own) and lane option (RANDOM / RANDOM
  2P). Each side has its own judge, gauge, score and keysound voices (one side's keysounds or HCN muting never
  cut the other's); a player who fails stops (FAILED over their side) and the other plays on; the play ends
  when both have finished or failed. The results show both sides and the winner. Nothing is saved: no
  scores, lamps, replays or play counts. Controllers: press a button on the 1P controller while the chart
  loads (the order they were connected in can swap them). On one keyboard, many keyboards miss keys when
  several are held (rollover). With touch controls, VS needs a tablet-sized screen (two control sets, each
  player's START / SELECT on their side); phones refuse it.
- **IR**: rankings and score submission over a JSON protocol ([docs/IR_PROTOCOL.md](docs/IR_PROTOCOL.md)).
- **Audio**: low-latency output, keysounds scheduled to the sample, WAV/OGG/MP3/FLAC. On Windows, WASAPI of its own:
  SHARED (the default) at the small periods of IAudioClient3 where Windows 10+ and the driver offer them (the buffer
  size setting rounded to one the engine takes), else the usual shared stream; EXCLUSIVE
  (`SETTINGS › DISPLAY & AUDIO › Output mode`, on restart) event driven at the device's smallest period, in the mixer's rate as float, 24-bit or 16-bit,
  else a format of the device's own, and back to shared with a message when the device refuses. ASIO/JACK feature
  flags for cpal.
- **Score import** (`SETTINGS › LIBRARY › Import scores`): LR2's (`LR2files/Database/Score/<player>.db`, filed
  under the LR2 judge preset), beatoraja's (`player/<name>/score.db`, beatoraja preset; LR2oraja's under LR2 when an
  LR2oraja program is in the install folder). For the charts in the library: the better lamp, EX score (with its
  judgements) and max combo, the lower min BP, and the play and clear counts as the higher of the two (not summed,
  so importing again changes nothing). Courses and charts not in the library are counted and left out.

## Skins

Play, song select and results are all drawn by skins, chosen in `SETTINGS › SKINS`.

- **Built-in default skins**: landscape and portrait layouts for every mode, original art drawn from shapes and
  the Inter font by `tools/gen_default_skins.py`. Edit the script and run it to make
  `crates/vibebms/assets/builtin/skins/default/` again.
- **Touch perspective** (a built-in play skin for every key mode, chosen in `SETTINGS › SKINS` like the others):
  each player's lanes are a fan seen in perspective, narrow at the far end and wider toward the judge line, notes
  growing as they come. The judge line is at three quarters of the screen's height and the lanes reach near its
  top; the keys are about a tenth of the screen wide at the line, and the line between the middle keys (7 keys:
  3|4, 5 keys: 2|3; 2P mirrored) stands upright in the middle of the screen, so the keys sit under the hands and
  the scratch, the wider red-tinted lane at the outer edge (left for 1P, right for 2P), leans out to its side (its
  bottom may run off the screen). The panels go in the top corners beside the lanes. Double play shows two fans
  side by side, meeting at the bottom. The hi-speed and green number mean what they do on flat lanes: notes travel
  the lane at the same rate, and the perspective only changes where a distance is on the screen (SUDDEN+, HIDDEN+
  and LIFT cover the same share of the lane, which looks smaller at the far end). With touch controls the lanes
  themselves are the controls, in the bottom half of the screen (the judge line in its middle), in zones that
  fill the width like a beatmania controller's keys: at the judge line each key's zone is its lane; between a
  white and a blue key the boundary bends, so the white key is wider below the line and the blue key above it.
  The outer zones reach the screen's edges (in double play, the middle) and the gap beside the scratch is shared
  by its neighbours; fingers can slide between lanes, the scratch lane takes taps and sideways swipes (a change of
  direction is a new scratch), and START / SELECT sit in a top corner. The zones are not drawn unless the skin's
  TOUCH AREA option (`SETTINGS › SKINS`, the skin's options) shows them at 25-100 %. There is no strip of controls: the play
  screen fills the screen. `SETTINGS › CONTROLS` sets the perspective (0 % is flat), how far up the lanes reach and
  the touch area's height (`[perspective]` in the config; a touch area left at an older version's default moves
  to the new one). The skin's canvas takes the screen's shape: objects in the JSON carry a vibeBMS
  `"anchor": [x, y, w, h]` (the share of the added width or height they move and grow by), and
  `"perspective": true` makes the game draw the note field as fans (a skin's options with ops 960-964 set the
  touch zones' opacity: 0, 25, 50, 75, 100 %; a skin without them draws them at the touch controls' opacity).
- **LR2 skins** (`.lr2skin`): put them under `skins/` (`LR2files\Theme\…` paths are taken from `skins/`). Images,
  numbers, text, `#LR2FONT`, gauges, notes, judgements, bar graphs, sliders, buttons, panels, the song list, result
  graphs, `#CUSTOMOPTION`/`#CUSTOMFILE`, DxLib `.dxa` archives.
- **beatoraja skins**: JSON (`.json`) and Lua (`.luaskin`, embedded Lua 5.4 in a sandbox; the web build too).
  `main_state`, `timer_util`, `event_util` and `skin_config` are provided, and conditions, timers, values and
  events given in Lua are evaluated every frame. Graphs (`gaugegraph`, `judgegraph`, `bpmgraph`,
  `timingdistributiongraph`, the timing visualizer) and blend modes are supported. Text outline and shadow are
  drawn as beatoraja draws them: TrueType and plain `.fnt` fonts get the shadow only (the text in half its colour
  at the offset), distance-field `.fnt` fonts (font `type` 1 / 2) the outline, shadow colour and softness too.
- **Battle skins** (types 12 / 13, "7KEYS BATTLE" / "5KEYS BATTLE" in `SETTINGS › SKINS`; a 14 / 10 key skin
  can be chosen there too): drawn when both sides of a battle are shown (G-BATTLE, VS). The built-in ones have a
  gauge per side and the opponent as the target. LR2 `#SRC_GROOVEGAUGE` with 1 in its first field (vibeBMS JSON:
  a gauge destination's `"side": 1`) is the second side's gauge. When there is an opponent: numbers 120-136 are
  its values as LR2's 2P (score, EX, rate, combo, max combo, notes, gauge, next rank, PG..POOR, total rate),
  108 is 1P minus 2P and 121 / 151 / 128 / 153 / bar graphs 114 / 115 its EX score as the target; timers 43 / 45
  / 49 / 144 its gauge rising, full, full combo and last note (42 / 143 the first side's); options 44 / 45 its
  groove / survival gauge, 56 / 57 its AUTO SCRATCH, 210-217 its DJ level, 352 / 353 / 354 the first side
  winning / losing / level. A skin's 2P judge region shows the opponent's judgements; a single play skin shown
  over a battle (a phone held upright) never does. Results: numbers 120-136 and options 352-354 likewise,
  vibeBMS option 9350 "a battle", text 9351 the opponent's name, numbers 9376 / 9370 its miss count and lamp.
- Player totals for skins (numbers 17-19, 30-37, 333): plays and clears are summed over the charts' records;
  judgements and time played are counted after every recorded play (not autoplay or replays). Judgements from
  before vibeBMS kept them count each chart's best play only; time played starts at zero.
- **Skin downloads**: vibeBMS bundles no third-party skins. `SETTINGS › SKINS › GET SKINS` lists the skins in
  [`skins/catalog.json`](skins/catalog.json) (read from this repository, the copy compiled in until then) and
  downloads, unpacks and installs one into `skins/` from its author's own link (Google Drive, Dropbox, MEGA, GitHub,
  plain servers), replaces it with a newer version, or deletes it. Each entry names its author, scenes, resolution,
  homepage and the terms its author states. The web build keeps them in the browser's storage, but does not unpack
  skins over 400 MB.
- A skin's custom options and offsets are on its screen's page in `SETTINGS › SKINS` (Enter on the screen's row).
- **Key config, skin select and sound set** (skin types 8, 9 and 10): with a KEY_CONFIG skin chosen, key `6` in song
  select (or the skin's key config button, or `SETTINGS › CONTROLS`) opens the key config screen it draws: ↑↓ or
  the turntable pick a key, ←→ the mode (7 / 14 keys, 9 keys, 5 / 10 keys), Enter, a white key or a tap on the key
  waits for a key, controller button or turntable axis to bind, Delete clears it, Esc / SELECT / BACK leaves (it
  saves as it goes). beatoraja's rows are drawn over beatoraja skins as beatoraja does; LR2 skins use their key
  buttons (101-136, 140-143, slots 150-159; Lunatic Vibes' texts 300-384 too). Without such a skin, `6` opens the
  settings' CONTROLS. With a SKIN_SELECT skin chosen, key `7` (or the skin button) opens the skin select screen:
  ↑↓ the skin or one of its options, ←→ change it, PgUp / PgDn the skin type, with the skin's preview (beatoraja
  `skinpreview`, LR2 image 105); without one, `7` opens `SETTINGS › SKINS`. Mouse and touch use the skins'
  buttons, and both screens have a BACK button.
- **Sound sets** (`SETTINGS › DISPLAY & AUDIO`, or SOUND SET in the skin select screen): the song select music, the decide
  jingle, folder / option clicks and result fanfares. A set is a folder of beatoraja's sound files (`select.wav`,
  `decide.wav`, `f-open.wav`, `o-change.wav`, `clear.wav`, ...; any audio format) or an LR2 sound set (`.lr2ss`),
  under `sounds/` or `skins/`; without one, a few short built-in clicks (no music). `SETTINGS › DISPLAY & AUDIO › System volume`
  sets their volume.
- vibeBMS extensions for skins: song select option text `9000+i` (value) and `9100+i` (name) and click events
  `9000+i`; result events 9500 (retry), 9501 (continue) and 9502 (watch the replay); song select events 9900
  (parent folder), 9901 (watch the chart's replay) and 9905 (practice the chart); number 9425 is the record's min CB (fewest combo breaks). A graph with
  `"style": "vibebms"` is drawn as the built-in skins draw it. START / SELECT during play turn on timers 21 (panel
  open) and 31 (closed), and slider / graph value 9400 is the hi-speed position (0.5–10.0). For skins with no object
  using timer 21, the app draws a hi-speed panel beside the lanes.

## Web version

Build it with `tools/build_web.sh` and serve `web/` over HTTP. The page's buttons take a folder, files or song pack
`.zip`s (for iPad Safari), which are scanned and played in the browser; nothing is uploaded anywhere. Song packs
(`.zip`, `.7z`, `.rar`, `.lzh`, picked on their own or in a folder) are unpacked as they come in, into a folder of
their name. An archive already kept in the browser's storage (one picked inside a folder before) is read as on the
desktop, all four formats; the page reads the archive into memory to scan it or to unpack a song from it (a song
of up to 1 GB), and lets it go again. Settings and
records are kept in localStorage.

- Songs are kept in the browser (its Origin Private File System) and are there again the next time the page opens.
  `SETTINGS › LIBRARY` shows how much room they take of what the browser allows, asks the browser to keep them for
  good, lists the song folders by size with a delete for each, and deletes those not played for 30 days or all of
  them; in song select, Delete removes the chosen song's folder (each delete asks for a second press). Add the page to
  the home screen: it runs full screen, and Safari, which may clear a site's storage after 7 days without a visit,
  keeps it.
- CI publishes a web build to the `gh-pages` branch on every push (the default branch at the site's root, other
  branches under `preview/<branch>/`). Turn on GitHub Pages (Settings › Pages › `gh-pages` / root) to open it.
  The repository variable `WEB_THIRD_PARTY=1` turns the third-party mirrors on by default in that build.
- A separate chart added on its own (📦) goes into its song's folder: the folder with the same title and artist that
  holds every sound it needs. When that cannot be decided, the page asks which song it is for.
- The browser paces the frames at the display's refresh rate, except Safari, which keeps pages at 60 frames a
  second unless "Prefer Page Rendering Updates near 60fps" is turned off (macOS: Develop › Feature Flags; iOS and
  iPadOS: Settings › Apps › Safari › Advanced › Feature Flags).
- Browsers have no system fonts for the page, so CI puts Noto Sans CJK (JP/KR, SIL OFL) into `web/fonts/`. When
  building yourself, add CJK fonts there and list them in `fonts.txt`.
- Difficulty table servers do not allow CORS, so the web build reads tables (page, header, data) through the vibeBMS
  proxy (`proxy.vibebms.workers.dev`) each time; tables you add work the same way. IR works when its server allows
  CORS.

## Layout

```text
crates/vibebms-core     parsing, timing, judging, gauges, scores (no platform dependencies)
crates/vibebms-audio    decoders and mixer
crates/vibebms-ir       IR client
crates/vibebms          the app (winit + wgpu, skins, scenes)
crates/vibebms-web      web entry point
crates/vibebms-android  Android entry point
```

Difficulty tables, song downloads, unpacking and placing separate charts come from
[BMSeeker](https://github.com/vibeBMS/BMSeeker) (a library and a CLI).

## Not implemented yet

Adapters for particular IR services (such as MochaIR).

## License

`MIT OR Apache-2.0` (LICENSE-MIT, LICENSE-APACHE). The embedded Inter font is SIL OFL 1.1
(`crates/vibebms/assets/fonts/OFL.txt`). The logo's letters (`assets/logo/`, made by `tools/gen_logo.py`) are Inter
converted to outlines. Downloaded `.rar` files are unpacked with
[unrar-rs](https://github.com/scryer-media/rarpar/tree/main/crates/unrar-rs) (Apache-2.0, with the UnRAR license
restriction); its RAR engine is based on the UnRAR source, whose terms are:

> UnRAR source code may be used in any software to handle RAR archives without limitations free of charge, but cannot be used to develop RAR (WinRAR) compatible archiver and to re-create RAR compression algorithm, which is proprietary. Distribution of modified UnRAR source code in separate form or as a part of other software is permitted, provided that full text of this paragraph, starting from "UnRAR source code" words, is included in license, or in documentation if license is not available, and in source code comments of resulting package.

Lua's pattern matcher (ported) and the BeMusicSeeker fork's rules (through bmseeker) are MIT; their notices are in
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). The judging, gauge and skin rules follow the behaviour of beatoraja,
LR2oraja and LR2 (see [docs/RULES.md](docs/RULES.md)), but no code is taken from these players: the judge engine,
judge windows, gauge damage rules, chart metrics, skin path matching and the SDF text shader were written from a
behaviour specification and tests, without reference to their code. No charts or music are included.
