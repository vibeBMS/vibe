# vibeBMS manual

How to use vibeBMS: its data folder, command line, controls, settings and features. The rules a play is judged and
recorded by are in [RULES.md](RULES.md); skin support is in [SKINS.md](SKINS.md); building is in
[BUILDING.md](BUILDING.md).

Contents: [Data folder](#data-folder) · [Command line](#command-line) · [Controls](#controls) ·
[Play options](#play-options) · [Hi-speed](#hi-speed) · [Controllers](#controllers) ·
[Touch controls](#touch-controls) · [Charts and formats](#charts-and-formats) ·
[Songs in archives](#songs-in-archives) · [Display](#display) · [Audio](#audio) ·
[Phones and tablets](#phones-and-tablets) · [BGA and images](#bga-and-images) · [Play extras](#play-extras) ·
[Target and rivals](#target-and-rivals) · [Profiles](#profiles) · [Languages](#languages) ·
[Difficulty tables and downloads](#difficulty-tables-and-downloads) · [Bulk downloads](#bulk-downloads) ·
[Third-party sources](#third-party-sources) · [Event mirror](#event-mirror) ·
[Replays and ghosts](#replays-and-ghosts) · [Practice](#practice) · [G-BATTLE](#g-battle) · [VS](#vs) · [IR](#ir) ·
[Score import](#score-import) · [Streaming](#streaming) · [Skins and sound sets](#skins-and-sound-sets) ·
[Web build](#web-build)

## Data folder

Settings, scores and caches live in the data folder. By default it is the executable's folder, so the whole
install can be copied around. `VIBEBMS_ROOT` sets another location.

```text
<data>/config/config.toml      settings (edited in the game; editing by hand works too)
<data>/config/profiles.toml    the profiles and the one in use (see Profiles)
<data>/scores/scores.json      play records
<data>/scores/player.json      totals over all plays (judgements, time played)
<data>/scores/favorites.json   favourite songs and charts
<data>/scores/replays/         replays
<data>/profiles/<id>/          another profile's config/config.toml and scores/ (the same files)
<data>/cache/                  song scan, archives' file lists, difficulty table and chart loudness caches (made again if deleted)
<data>/skins/                  LR2 / beatoraja skins
<data>/sounds/                 system sound sets (optional)
<data>/fonts/*.ttf|otf|ttc     fonts to use (optional)
<data>/downloads/              downloaded archives
<data>/screenshots/            screenshots
```

BMS folders inside the data folder are scanned automatically; the folders above are never scanned for charts.

| where | data folder |
|---|---|
| desktop executable | its own folder; if that cannot be written to, `vibeBMS` in the user data folder (`%APPDATA%`, `$XDG_DATA_HOME` or `~/.local/share`) |
| macOS app | the folder that holds the `.app`; in an `Applications` folder, or where that folder cannot be written to (a disk image), `~/Library/Application Support/vibeBMS` |
| Flatpak | `~/.var/app/<app id>/data/vibeBMS` (the nightly's app id is `io.github.vibebms.nightly`) |
| Android | `Android/data/io.github.vibebms/files` (the nightly: `Android/data/io.github.vibebms.nightly/files`); copy BMS folders there over USB |
| iOS / iPadOS | the app's `Documents` folder, visible in the Files app |
| web build | the browser's storage (see [Web build](#web-build)) |

## Command line

The desktop executable takes a chart to play at start, options for this launch and a few subcommands
(`vibebms --help` lists them).

| argument | effect |
|---|---|
| `<chart>` | play the chart at start |
| `--autoplay`, `-a` | autoplay it; without a chart, AUTOPLAY on in song select |
| `--ghost=best\|last` | play it as G-BATTLE against that replay |
| `--vs` | play it as VS |
| `--gauge=HARD`, `--random=MIRROR`, `--hs=3.25`, `--judge=lr2`, `--ln="FORCE CN"` | options for this launch: labels as the game shows them, or the config's names, in any case |
| `--set <section.key>=<value>` | any setting of `config.toml` for this launch (`--set play.sudden=0.3`, `--set display.show_fps=true`) |
| `--print-effective-options` | print the `[play]` options these make, and exit |
| `vibebms songs scan` | scan the BMS folders into the song cache, as F12 does |
| `vibebms songs list [--json]` | print the library as last scanned: a line per chart (MD5, key mode, level, title, path), or the cache's records as JSON |
| `vibebms export video ...` | render a chart to a video file (below) |
| `vibebms -P -N<measure> <chart>`, `vibebms -S` | the chart viewer for BMS editors (below) |

Options given here are never written to `config.toml`; a setting changed in the game meanwhile is saved as usual. An
option that does not read stops with its error and exit code 2.

### Video export

`vibebms export video <chart> [--replay <best|last|1-4>] [--out <file.mp4>] [--size <W>x<H>] [--fps <fps>]
[--duration <seconds>]` renders the chart's autoplay, or its replay in that slot, to a video file.

- It runs from the play's lead-in to the end of the play as shown before the result, or for `--duration` seconds.
  A replay is shown as watched from song select, at 100 %.
- Frames are drawn offline at a fixed step of the play's clock: 60 a second and 1280x720 unless `--fps` and
  `--size` say otherwise (even sizes). The play skin is the one chosen for the chart's mode, with the settings of
  `config.toml` and touch controls off.
- Drawing needs a GPU or a software renderer (Mesa's lavapipe, say); without one it says that drawing without a
  window is not available.
- The chart's sounds are mixed offline at the configured volumes, at the sample rate of its sound files (the most
  common of its first ones, else 44.1 kHz).
- Frames and sound go to the installed `ffmpeg` (the one video BGA uses), which encodes them with its default codec
  for the file's container (`.mp4`, `.mkv`, `.mov`, `.webm`, ...). Videos in the BGA and the skin wait for each of
  their frames.
- The progress is printed as it goes. Ctrl-C stops it and leaves no file behind (exit code 130). The default file is
  the chart's name with `.mp4`, in the current folder.

### Chart viewer

vibeBMS takes uBMplay's command line, as BMSE, iBMSC, BmsONE, woseq and BMS Maker 3 call their external viewer.
`vibebms -P -N<measure> <chart>` autoplays the chart from that measure (0 based, as the editors count; `-N 3` too;
without `-N` from the start), and `vibebms -S` stops it.

| editor | viewer setting |
|---|---|
| BMSE, iBMSC | the vibeBMS executable; they send uBMplay's arguments |
| BmsONE | play arguments `-P -N$(measure) "$(filename)"`, stop argument `-S` |
| BMS Maker 3 | play arguments with its measure and file name placeholders |
| woseq | as it is |

The first `-P` starts vibeBMS as the viewer, and it stays running. A later `-P` or `-S` is a short process that hands
the request to the viewer over a loopback port (127.0.0.1:39177) and exits. The viewer reads the chart again and
plays it from the measure (as practice starts a section, with what was sounding there), or stops and goes back to
song select, and comes to the front. The play shows `VIEWER #<measure>` and is never recorded. Any program on the
computer can reach that port, so the viewer only takes what `-P` sends: the full path of a chart file that is there,
on a disk of this computer (not a network share's `\\host\...`).

## Controls

Keyboard defaults are beatoraja's:

| mode | keys |
|---|---|
| 1P 7 keys | `Z S X D C F V`, scratch `LShift` / `LCtrl` |
| 2P 7 keys | `M K , L . ; /`, scratch `RShift` / `RCtrl` |
| 9 keys (PMS) | `A S D F G H J K L` |
| 4 / 6 / 8 keys | `D F J K` / `S D F J K L` / `A S D F J K L ;`; controllers use their 7 key buttons and turntable, at the chart's lanes |

### Anywhere

| key | action |
|---|---|
| F1 / Tab | settings |
| F11 | full screen |
| Print Screen (F13 on Apple keyboards) | screenshot, also song select's SCREENSHOT button |

A screenshot is saved as `screenshots/YYYYMMDD-HHMMSS_<title>_<difficulty>_<lamp>_<EX>.png` in the data folder; on
Android and iOS in the app's storage, on the web as a download. The frame is copied on the GPU and read back a few
frames later, so play does not stall.

### Song select

| key | action |
|---|---|
| ↑ ↓ | move |
| Enter, → or a white key | decide: open a folder, play a chart |
| ← or a blue key | close the folder |
| Backspace / Esc | back |
| `/` | search |
| F2, F4–F9 | play options; GAUGE has no key of its own (START + 3 and the OPTIONS buttons) |
| F3 | show the chart's file in the system's file manager (desktop) |
| T | read the song's text files; O (or OPEN) then opens the file shown in the system's text viewer (desktop) |
| U | open the chart's web page in the browser: its `#URL`, a bmson `info.url`, else its link in a difficulty table (http(s) only) |
| B | SAME FOLDER: the charts in the chosen chart's folder |
| F12 | rescan the library |
| I | the chart's IR page |
| P | watch the chart's replay, or a course's (see [Replays and ghosts](#replays-and-ghosts)) |
| E | practice the chart (also the PRACTICE button) |
| 6 / 7 | key config / skin select |
| 1 | key mode filter: ALL / 7 / 14 / 9 / 5 / 10 / 4 / 6 / 8 KEYS |
| 2 | sort: title, level, clear, score rate, miss count, last played, newest, BPM, artist, notes |
| 4 | difficulty filter: ALL / BEGINNER … INSANE |
| 8 / 9 | favourite song (every chart in its folder) / chart |
| 0 | RANDOM SELECT's choice: ANY, NOT CLEAR, NOT HARD, NOT FC |
| Delete | (web build) delete the chosen song's folder |
| holding START (`Q`) / SELECT (`W`) | the play option panels (below) |

- On Linux, F3 selects the file in a file manager that takes the freedesktop `FileManager1` call, else opens the
  folder. F3, U and B are also in the default skin's TAG EDITOR panel.
- The filters and the sort are kept in `[select]` and are also on the OPTIONS buttons. Favourites are listed in the
  FAVORITE folder at the root and marked ♥; on touch, the OPTIONS buttons FAV CHART / FAV SONG set them.
- Virtual folders at the root, after FAVORITE, all with the filters and by the records of the judge preset chosen:
  LAMP UPDATE and SCORE UPDATE (charts whose lamp a play raised to a clear, or whose best EX score a play raised, in
  the last 7 days), CLEAR TYPE (a folder per lamp), SCORE RANK (per DJ level, NO PLAY last), LEVEL (per level) and
  NEW (charts whose file came or changed in the last 7 days).
- RANDOM SELECT, the second bar of each folder with charts, plays one of them drawn at random, as filtered. 0 (or
  its OPTIONS button) chooses from which, by the lamps of the judge preset chosen.
- Entering a folder puts the cursor on the folder's own row; deciding on it closes the folder (as in LR2).

Holding START or SELECT shows beatoraja's option panels, in its key layout:

| panel | keys |
|---|---|
| START | 1▼ 2▲ RANDOM, 3 GAUGE, 4 DP OPTION, 5 HS-FIX, 6▲ 7▼ 2P RANDOM, turntable TARGET |
| SELECT | 1 EXPAND JUDGE, 2 CONSTANT, 3 JUDGE AREA, 4 LEGACY NOTE, 5 MARK NOTE, 6 BPM GUIDE, 7 NO MINE; the turntable FREQ |
| both (or number key 5) | 1 BGA, 2 GAUGE AUTO SHIFT, 3 judge timing auto-adjust, 4·6 green number −/+ (repeats while held), 5·7 judge timing −/+ |
| number key 3 | LN mode |

### Play

| key | action |
|---|---|
| ↑ ↓ | hi-speed |
| PgUp / PgDn | SUDDEN+ |
| ← → | judge offset |
| Esc | quit |
| START + white key / blue key | hi-speed −0.25 / +0.25 |
| START + turntable | lane cover (LIFT without a cover) |
| START twice | toggle the cover |
| SELECT + keys / turntable | hi-speed ±0.01 |
| START + SELECT for a second | quit |

### Results

| key | action |
|---|---|
| R / F5 | retry |
| P | watch this play again |
| 1–4 | keep its replay in that slot (the default skin's SAVE 1–4) |

## Play options

The lane options, assists and chart modifiers are set in song select (F-keys, the panels, the OPTIONS buttons). What
each does to a play, and which count as assists, is in [RULES.md](RULES.md) ("Lane options", "Chart modifiers",
"Clear lamps").

| option | values | where |
|---|---|---|
| lane option | OFF, MIRROR, RANDOM, R-RANDOM, S-RANDOM, H-RANDOM, SPIRAL, ALL-SCR, RANDOM+, S-RANDOM+, F-RANDOM, MF-RANDOM, TRAINER | START panel, F-keys |
| DP OPTION | OFF, DP FLIP, BATTLE, BATTLE AS, G-BATTLE MY BEST / LAST / SLOT 1–4, VS | START panel, F1 |
| assists | EXPAND JUDGE, JUDGE AREA, MARK NOTE, BPM GUIDE, judge timing auto-adjust, AUTO SCRATCH, LEGACY NOTE, NO MINE | SELECT panel |
| chart modifiers | SCROLL REMOVE, MINE ADD (RANDOM, NEAR, BLANK), LONGNOTE ADD (LN, CN, HCN) | ASSIST panel (hold SELECT) |
| FREQ | −12 to +12 semitones | hold SELECT and turn the turntable, or the ASSIST panel's FREQ row |
| LN MODE | AUTO LN / CN / HCN, FORCE LN / CN / HCN | key 3, F4, PLAY OPTION |
| KEY CONVERT | OFF, 7KEYS, 9KEYS | select option, after SAME FOLDER |
| TRAINER ORDER | turns the trainer order a lane at a time | select option |

- EXPAND JUDGE widens the windows (PG / GR / GD ratios in the settings); JUDGE AREA draws the judge windows above
  the judge line; MARK NOTE keeps notes hit early grey down to the line; BPM GUIDE shows BPM changes and stops.
- BATTLE and BATTLE AS play with AUTO SCRATCH on both sides.

Settings in `SETTINGS › GAMEPLAY`:

| setting | values | default | effect |
|---|---|---|---|
| Judge algorithm | COMBO, DURATION, LOWEST, SCORE | COMBO | the note a press takes when several are in reach (RULES "Judge presets") |
| Hi-speed type | CLASSIC, NORMAL, FLOATING, LR2 | CLASSIC | see [Hi-speed](#hi-speed) |
| Hi-speed change while playing | 0–1000 ms | 150 ms | how long a hi-speed change takes to reach the notes |
| ADVANCED › S-RANDOM interval | OFF, 40 MS, 120 HZ | OFF | how S-RANDOM keeps notes off the key just played (RULES "Lane options") |
| ADVANCED › Random trainer order | digits, one per key | | TRAINER's lane order, keys from the left: `3142576` puts key 3's notes on the first lane; `7654321` is MIRROR |

The judge preset (beatoraja, LR2, IIDX, or custom windows) and the gauge set are chosen in the settings too; each
preset keeps its own records and replays, and song select shows those of the preset chosen.

## Hi-speed

`SETTINGS › GAMEPLAY › Hi-speed type` chooses how the hi-speed is set:

| type | what is set | what follows |
|---|---|---|
| CLASSIC (default) | the multiplier, in steps of 0.25 | the green number (how long notes are visible), from the BPM and the lane cover |
| CLASSIC with HS-FIX (START / MAX / MAIN / MIN BPM) | the green number | the multiplier, from each chart's BPM |
| NORMAL | IIDX's normal hi-speed: levels 1 to 20, green numbers of the whole lane (1200, 1000, 800, 700, 650 … 300, 280, 260) at the HS-FIX BPM (MAIN without HS-FIX) | the hi-speed keys step a level |
| FLOATING | IIDX's floating hi-speed: the green number, kept through the lane cover, LIFT and the chart's BPM changes | the multiplier follows the BPM of the moment (before the start, the HS-FIX BPM) |
| LR2 | LR2's multiplier; with HS-FIX the speed at 150 BPM | the screen and skins show the setting; the lane cover leaves it as it is |

A change made while playing reaches the notes over the time `Hi-speed change while playing` sets, easing out; the
new hi-speed and green number show over the note field for a moment. A change the chart makes (FLOATING through a
BPM change) is not eased.

## Controllers

Controllers, IIDX controllers included, are bound in `SETTINGS › CONTROLS` by pressing their buttons. Analog
turntables work; their sensitivity and stop time, and a key chatter filter, are set there (RULES "Input filters").

- `1P controller` / `2P controller`: AUTO (the default: the order they were connected in, `Pad0` 1P, `Pad1` 2P) or
  one of the connected controllers. A chosen one is kept by its name and USB id (browsers give only the name) and
  found again at the next launch.
- A chosen controller that is not connected leaves its side to the next one in connection order. Two of the same
  model are 1P and 2P in the order they were connected. Choosing the other side's controller swaps them, as
  `Swap 1P and 2P controllers` does.
- Bindings follow the side, not the controller: `Pad0#3` is button 3 of whichever controller is 1P.
  `Connected controllers` lists each with its side.
- Each side has a second keyboard map, `1P keyboard sub` / `2P keyboard sub`: a second key for each key and scratch.

## Touch controls

`SETTINGS › CONTROLS` chooses the touch layout:

- **BM**: a turntable and two rows of keys; round pop'n buttons for PMS; a single row for 4 / 6 / 8 keys.
- **Single row** of keys.
- **Custom**: the layout editor (`KEY CONFIG › Touch controller layout`) moves and resizes the buttons for each key
  mode and orientation (drag them, drag their corners or pinch them), flips a layout left to right, and sets the
  height of the controls (drag their top edge). Custom layouts are used as placed, whatever the play side.

Portrait or landscape single play and landscape double play work, with multi-touch and sliding. A touch between keys
plays the nearest one. Keys are sized like an arcade's; the screen size is guessed from the device and can be set.
The START / SELECT buttons sit in the letterbox beside the play screen, on the turntable's side (or the side chosen),
and open the panels above. A key whose release the browser never delivers (an iPad's home bar dragged with a key
held) is let go once no finger is on the screen.

The touch perspective play skin uses its lanes as the controls ([SKINS.md](SKINS.md), "Touch perspective").
`SETTINGS › CONTROLS` sets its perspective (0 % is flat), how far up the lanes reach and the touch area's height
(`[perspective]` in the config); the skin's TOUCH AREA option (`SETTINGS › SKINS`) shows the zones at 25–100 %.

## Charts and formats

- BMS, BME, BML, PMS and BMSON, in 5, 7, 10, 14 and 9 keys (`.pms` in the standard or the BME-type layout), and
  Qwilight-style 4, 6 and 8 keys (RULES "4, 6 and 8 keys").
- `#RANDOM` / `#SWITCH`, BPM changes (negative BPM as LR2: RULES "Negative BPM"), STOP, `#SCROLL` and beatoraja's
  `#SPEED` (RULES "Scroll"), long notes (`#LNTYPE`, `#LNOBJ`, `#LNMODE`: RULES "Long notes") and mines.
- Text encodings: Shift_JIS, EUC-KR and UTF-8, detected.
- The scan reads new and changed charts several at a time.
- Not read (no real chart is so): a chart file over 32 MiB, a chart whose last note or sound is past 24 hours, one
  with more than 4,000,000 objects. A header's text is read to 1024 bytes, a number to a thousand million.
- A chart or an archive the scan was reading when the game stopped is read on its own by the next scan, before the
  others. If the game stops again while it is read, it is left out from then on (the log names it) until the file
  changes or another version of the game scans.

## Songs in archives

A `.zip`, `.7z`, `.rar`, `.lzh` (`.lha`) or `.tar` (`.tar.gz`, `.tgz`; told by its bytes too) in the library is
played without unpacking it yourself.

- It is scanned as a folder, its charts in sub-folders too. Its charts are named by the archive, a `!` and their path
  in it: `bms/packs/pack.zip!/Song/chart.bme`. That path also plays from the command line. Song select lists an
  archive right in a library folder as a folder of its own.
- An archive is read when it is new or its size or time changed: its list of files goes into `cache/archives.json`,
  and only its charts are read. A 7z, RAR, LZH or tar (solid ones and gzip-compressed tars included) is decoded from
  the start once. An unchanged archive is not opened by the scan.
- Playing a chart unpacks its song (the folder that holds the chart, with its sub-folders) once: into memory, or
  into `cache/unpacked/` (emptied at start) when it is larger than 512 MB (32 MB on Android and iOS). It is kept
  while the song stays chosen and let go when another is chosen.
- The song preview, stage file, banner, text files and videos work as for a folder. Outside a play, a zip's files are
  read where they are; a 7z's, RAR's, LZH's or tar's song is unpacked on its first file read.
- Nothing is written into an archive: song downloads take a song in an archive for not installed, and a separate
  chart for it is not put into it. A chart that is also a file of its own (the archive unpacked beside it) is listed
  once, as the file.
- Not read: archives in an archive, encrypted archives, tars compressed other than with gzip (`.tar.xz`, `.tar.bz2`,
  `.tar.zst`).
- Limits, as an archive can say or hold anything. An archive is left out (the log says why; it is read again when it
  changes) when its charts are more than 256 MB in all (64 MB on phones and the web), when it lists more than
  1,000,000 files (250,000), or when a 7z, RAR, LZH or tar decodes to more than 64 GB. A chart larger than a chart
  file may be (32 MB) is passed by. A song larger than 4 GB, or than the disk has room for with 256 MB to spare, is
  not unpacked.
- A file's name in an archive is the one it would have unpacked on any system: `_` stands for `< > " | ? *` and
  control characters, a dot or space at the end of a name is dropped, and a name Windows keeps for a device (`CON`,
  `NUL.wav`, `COM1`) gets a `_` before it. A chart finds such a file under that name only.

## Display

Settings in `SETTINGS › DISPLAY & AUDIO`:

| setting | values | default | effect |
|---|---|---|---|
| Present mode | SMOOTH, LOW LATENCY, UNLIMITED | SMOOTH | SMOOTH: vsync with two frames queued, each drawn for when it will be on the screen; even motion at the display's refresh rate. LOW LATENCY: the newest frame at each vblank without tearing where the GPU has mailbox presentation, else as SMOOTH (macOS, iOS). UNLIMITED: as many frames as the GPU makes, with tearing |
| Show FPS | on, off | off | shows the frame rate reached |
| Latency overlay | on, off | off | shows in the top left the median, 95th percentile and maximum of the frame time, the audio output's callback interval (not on the web) and the input delay |
| ADVANCED › Graphics API | AUTO, Vulkan, DX12, OpenGL, Metal | AUTO | applies at the next launch (not on the web) |

- Android asks for the screen's highest refresh rate; the iOS app allows ProMotion iPhones above 60 Hz.
- The input delay is an input event's time to its handling by the screen: on the web, on iOS and for controllers from
  the event's own time, else from when the window got it. With the overlay off, nothing is measured.
- Graphics API: AUTO lets wgpu choose (Vulkan, Metal, DX12, then OpenGL). The choices are the platform's: Vulkan, DX12
  and OpenGL on Windows, Vulkan and OpenGL on Linux and Android, Metal on macOS and iOS. If the one chosen does not
  start, the others are tried in AUTO's order; the one that started is kept as the setting and said once in a message.
  A choice the platform lacks starts as AUTO. The `GPU` line below it shows the device and API in use.
- The start's phases (event loop, window, graphics, audio, library, tables and skins, song select, first frame) are
  timed in the log (`startup:` lines).

## Audio

Output is low latency, with keysounds scheduled to the sample; WAV, OGG, MP3 and FLAC are read. Settings in
`SETTINGS › DISPLAY & AUDIO`:

| setting | values | default | effect |
|---|---|---|---|
| Output mode (Windows) | SHARED, EXCLUSIVE | SHARED | on restart; see below |
| Song preview | LOOP, ONCE, OFF | LOOP | ONCE plays it once, then the select music again |
| Mute when inactive | on, off | on | fades out in 0.1 s while vibeBMS is not the app in use |
| Normalize loudness | on, off | on | every chart plays at the same loudness |
| Loudness target | −24 to −6 LUFS | −12 | |
| Normalize sound set music | on, off | on | the select music, the decide jingle and `.loop` music brought to the target too |
| System volume | | | the sound set's volume |

- **WASAPI** (Windows, vibeBMS's own): SHARED uses the small periods of IAudioClient3 where Windows 10+ and the
  driver offer them (the buffer size rounded to one the engine takes), else the usual shared stream. EXCLUSIVE is
  event driven at the device's smallest period, in the mixer's rate as float, 24-bit or 16-bit, else a format of the
  device's own; it goes back to shared, with a message, when the device refuses.
- ASIO and JACK output are build features ([BUILDING.md](BUILDING.md)).
- **Song preview**: the chart's `#PREVIEW`, else a `preview*` sound in its folder, else one made of the chart's BGM
  and keysounds over 15 s (10 s on the web and mobile) from its first dense part. It is made in the background (on
  the web once the cursor has rested 1.2 s); the last few made are kept in memory.
- **Mute when inactive**: out of play, vibeBMS is inactive when another app or window has the focus, or it is
  minimised, covered, in the background or (web) in a hidden tab or a page without the focus; the song preview starts
  again from its beginning when vibeBMS is back. In play, only a hidden, minimised or backgrounded vibeBMS is silent
  on the desktop and the web; on phones and tablets losing the focus silences play too. The play goes on (there is no
  pause). The in-app browser's window counts as vibeBMS.
- **Loudness normalisation**: a chart is measured the first time it is played, as its sounds are decoded: its BGM and
  every note's keysound autoplayed, into an ITU-R BS.1770-4 meter, which gives its integrated loudness and sample
  peak. The result is kept by the chart's SHA-256 in `cache/loudness.txt` (on the web in the browser's storage). In
  play the keysound and BGM buses are turned up or down to the target, at most 30 dB down and 12 dB up, and never so
  far up that the chart's peak passes the output's soft limiter. Song previews are measured and normalised the same
  way. A NONSTOP course is measured each time.
- The sound set's music is measured as it is first decoded; its other sounds (clicks, fanfares) and the system volume
  are not changed.

## Phones and tablets

- Touch controls turn on automatically ([Touch controls](#touch-controls)).
- The iOS app hides the status bar and, until the screen is touched, the home indicator. In play and the touch layout
  editor, a swipe from the top or bottom edge is the game's; the system's gesture needs a second swipe.
- The Android app hides the status and navigation bars: a swipe from an edge shows them for a moment.
- With Mute when inactive off, the iOS app's audio session keeps playing in the background.
- Songs a difficulty table lists are downloaded in the in-app browser, over the game screen.

## BGA and images

- **BGA**: image BGA with its layers and POOR layer (a BMS chart without a POOR layer shows its `#BMP00` on a miss, as
  LR2 and beatoraja do), and video BGA on the desktop with an installed `ffmpeg` (`VIBEBMS_FFMPEG` sets its path, else
  `ffmpeg` on the `PATH`). BGA ON, AUTO or OFF (SYSTEM OPTION key 1), as beatoraja's: AUTO shows it only when
  watching (autoplay, a replay).
- **Images**, in charts and skins: BMP, PNG, JPEG, GIF, WebP, TGA, DDS and CIM.
- Animated GIF, WebP and APNG play at their own frame times: a BGA image from when it is first shown, a skin's images
  from when the skin loaded, a chart's stage file, banner and `#BACKBMP` from when they loaded. The file's loop count
  is kept, then the last frame stays. A GIF or WebP frame of 10 ms or less lasts 100 ms, as in browsers.
- Each animation's frames are kept in memory, up to 128 MB for one image and 512 MB for all (32 and 96 MB on the web
  and mobile). One with more keeps every second frame (then every fourth, ...), each shown as long as those it stands
  for, so it plays at its speed.

## Play extras

Settings in `SETTINGS › GAMEPLAY › PLAY EXTRAS`. None changes how a play is judged or recorded.

| setting | default | effect |
|---|---|---|
| FAST / SLOW threshold | 0 (any) | the play skin's FAST and SLOW show only for a judgement more than this many ms off |
| GUIDE SE | off | a sound for each judgement as it is made (see below), at the system sound volume; G-BATTLE's opponent is silent |
| Notes wait at the line | off | as LR2 draws them: a note not yet judged stops at the judge line once its time has come, until it is hit or missed |
| RESULT WAIT | off | the play goes on to 5 s after the chart's last note, BGM or BGA layer, instead of ending a second after the last judgement; not after a practice section, a quit or a fail |
| GOOD clears the miss BGA | off | a PGREAT, GREAT or GOOD takes the POOR image away at once; the next BAD or POOR shows it again |
| Keysound on a missed note | off | a note passed without a press plays its keysound as its POOR comes (a CN / HCN's head once, its end sound when held past its end) |

- GUIDE SE takes the sound set's files by beatoraja's names: `guide-pg`, `guide-gr`, `guide-gd`, `guide-bd`,
  `guide-pr` (an empty POOR) and `guide-ms` (a missed note), any audio format. For what the set has none of, it plays
  a built-in tone.
- RESULT WAIT, GOOD clears the miss BGA and Keysound on a missed note are LR2oraja Endless Dream's.

## Target and rivals

The TARGET option sets the pacemaker: the personal best, rank A–AAA, MAX- / MAX, an IR rival or a local rival.

- The IR rivals your enabled accounts follow come after BEST in the TARGET list (`RIVAL <name>`), then the other
  profiles (local rivals).
- The target is the EX score the IR has of theirs for the chart, or the other profile's best EX score of it under the
  LN mode and judge preset chosen (0 when there is none). It is paced at the share of the notes judged, as the IRs and
  the records give no per-note data.
- With TARGET MY BEST, the pacemaker is the best play's ghost: its EX score at the same note.

## Profiles

Several players can share one library, switched without restarting in `SETTINGS › SYSTEM › PROFILE` or the default
skin's PLAYER STATUS panel (PROFILES ›).

| each profile's own | shared by every profile |
|---|---|
| records: best scores and lamps, totals, favourites, courses and their records, the play history | the library and its caches |
| replays | difficulty tables, events and download settings |
| IR accounts, their keys and outbox | the skins and sound sets installed |
| play options and offsets, keys and bindings (4K / 6K / 8K too), touch layout, volumes, the skins chosen with their options and offsets | the app-wide settings (below) |

The app-wide settings belong to the machine and stay in the data folder's `config.toml` whichever profile changes
them: the window and graphics (`[display]`), the audio device, which controller is 1P and which 2P
(`[keys.controllers]`), Discord and OBS (`[discord]`, `[obs]`), the song folders, tables and download sources, the
language and the update check.

- The data folder's own files are the default profile. Another one is `profiles/<id>/` (its own
  `config/config.toml` without the app-wide settings, and `scores/`), listed with its name in
  `config/profiles.toml`. A profile folder copied into `profiles/` shows up by its folder's name.
- NEW PROFILE starts from the settings in use, without the IR accounts, and with no records. Choosing a profile goes
  back to song select with its settings, records and IR accounts.
- A profile can be renamed (its folder keeps its name). Deleting one asks for a second press and deletes its records,
  replays and settings; the default profile and the one in use are never deleted.
- Another profile is a local rival: read only, its lamps beside yours (`SETTINGS › ONLINE › Rival lamps beside yours`)
  and its EX score as TARGET.
- On the web each profile's settings and records are in the page's storage too (`profiles/<id>/`), apart from the
  library that BMSeeker's page shares.

## Languages

`SETTINGS › SYSTEM › LANGUAGE`: English, Korean, Japanese, Simplified and Traditional Chinese, applied at once. AUTO
(the default) picks the system's: the OS locale on the desktop, Android and iOS (`LANG` / `LC_ALL` / `LC_MESSAGES` on
Linux), the browser's language on the web; any other language is English. The setting is app-wide.

- Translated: the settings, messages, dialogs, the practice screen, the touch layout editor, the key config screen's
  own text, the play screen's hints, the loading screen and the result's notes.
- In English in every language: the terms BMS players use in English (EX SCORE, PGREAT, gauge and option names, LN /
  CN / HCN, BPM, judge presets), chart and skin text, the text skins read from vibeBMS (their beatoraja and LR2 values
  keep their English form), log messages and the command line. Some error details come from servers in English.
- The Chinese tables are machine-quality translations checked by hand (Traditional uses Taiwan terms); corrections
  are welcome. A text a table lacks shows in English.
- Fonts: the desktop puts the language's own CJK system font first (Noto Sans CJK KR / SC / TC, Malgun Gothic,
  Microsoft YaHei / JhengHei, PingFang SC / TC); the web build loads Noto Sans SC / TC only while the UI is in that
  Chinese. Glyphs are read from the font file as they are drawn, so a font takes memory for the characters on screen.

## Difficulty tables and downloads

Difficulty tables (bmstable format) list charts by level. The insane and normal tables, Satellite and Stella are
there from the start; others can be added in `SETTINGS › LIBRARY`.

- Charts a table lists that are not installed show dimmed. Choosing one downloads its song and its separate chart
  (差分) in the app. When the song is in the library already, only the chart is fetched; a message says when none of
  its links works.
- Google Drive, Dropbox, MediaFire, MEGA and plain archive servers download directly; a manbow work page is read for
  its download links.
- What a download holds is sorted before it enters the library, the same on the desktop and the web: a new song gets
  a folder of its own; a song the library has already is not added again (only files its folder lacks come in); a
  separate chart goes into its song's folder, or waits when two folders are as likely. The rules are in BMSeeker's
  [DESIGN.md](https://github.com/vibeBMS/BMSeeker/blob/main/DESIGN.md#into-the-library).
- Links that cannot be downloaded directly (OneDrive, uploaders, deleted files) open their download page: on the
  desktop, Android and iOS in the in-app browser, whose downloaded archive is unpacked into `bms/` before the chart's
  page opens. A download comes into the library only after you say yes to it, one at a time. On the desktop and iOS
  the file is saved aside first and the game asks when it has arrived, showing its name, the host it came from and
  its size (the browser's window is out of the way meanwhile): OK brings it in, anything else deletes it. On Android
  the question comes before the download. None over 16 GB is kept. A downloaded chart goes into its song's folder;
  when unsure, the app asks. The web build opens the system browser.
- Downloads are kept in `downloads/`.
- **Name song folders after their charts** (`SETTINGS › LIBRARY`, off by default): a downloaded song gets a folder
  named `[artist] title` from what its charts share, as BeMusicSeeker names one, instead of the song's title.

## Bulk downloads

A table's folder and each of its levels start with DOWNLOAD MISSING (n charts) while the library lacks any of their
charts; choosing it queues them all.

- The queue downloads as for a chart chosen alone (its links, the other links known for it, the song before its
  separate chart), two at a time, so hosts are not hammered. A chart whose song another of its charts is downloading
  waits for it.
- DOWNLOADS at the root (while the queue is not empty) lists every chart as QUEUED, DOWNLOADING, DONE, FAILED (with
  why), PAGE ONLY or CANCELLED, after CANCEL, RETRY FAILED and CLEAR FINISHED.
- PAGE ONLY: its links lead only to a page; choosing it opens the page. The queue never opens pages.
- Choosing a queued or downloading chart cancels it; a failed or cancelled one queues it again. The queue is kept
  until the game closes.

## Third-party sources

`SETTINGS › LIBRARY › Third-party sources`, off by default: other people's copies of dead uploaders and sites
(darksabun.club, bms.hexlataia.xyz), lookups of a chart's package by its MD5, and the community archive
(bms.kyouko.moe).

- Off, downloads use only the authors' own links (as the tables, BMS SEARCH and Stella list them) and the event
  mirror.
- What is found there by name only goes into the library if it holds the chart wanted, by its MD5.
- Turning them on asks once: they are other people's copies, not the authors' links; whether a site allows this and
  whether it is up differs from site to site; using them is your own responsibility (each site's terms, the rights
  of the songs' copyright holders). OK turns them on and is remembered (app-wide); Cancel leaves them off.
- A build that has them on by default, or a setting on that was never acknowledged, says the same once at start,
  with OK and Turn off.
- vibeBMS names itself on its requests and never poses as a browser; the web build sends the browser's own name. See
  BMSeeker's [Third-party sources and
  disclaimer](https://github.com/vibeBMS/BMSeeker#third-party-sources-and-disclaimer).
- Skin downloads follow the same setting ([SKINS.md](SKINS.md), "Skin catalog").

## Event mirror

The BOF events of [vibeBMS/BMS](https://github.com/vibeBMS/BMS) (BOF22 to BOF2004) show as folders in song select.

- For events mirrored as they were (BOF22, BOF21, BOF:TT, BOF:XVII, BOF:XVI), a work not installed is downloaded from
  the mirror and unpacked into `bms/<event>/`.
- For the others the app downloads from the authors' links. A link that needs a person on its page, or gives a page
  instead of an archive, opens that page.
- The web build works the same way, through the vibeBMS proxy (see [Web build](#web-build)).
- Other event lists or indexes can be added in the settings (`archives`).

## Replays and ghosts

Every play of your own (not autoplay) keeps a replay: what drove the judge, with the seed and play options that made
the chart, so watching it judges exactly the same (RULES "Replay formats").

- Each chart keeps its latest play's and its best EX score's replay, per LN mode and judge preset (`scores/replays/`;
  in the web build, in the browser), and four slots of your own: 1–4 on the results keep that play's replay in the
  slot, replacing what it held.
- A course keeps its replays under the course: a NONSTOP course's play, and each stage of an expert or class course
  (its latest, and the slots its results kept).
- Watch one with P on the results (that play) or in song select (P or REPLAY). SYSTEM OPTION's REPLAY picks which:
  BEST, ELSE LAST (the default), MY BEST, LAST or SLOT 1–4. On a course's bar it watches the course: an expert or
  class course's stages one after another, each its own replay of that slot, as far as there is one.
- REPLAY SPEED (25–300 %, steps of 25) watches it slower or faster: the clock, the scrolling, the BGA and the sounds
  run at that speed while the judge gets every input at its own chart time, so the judgements, score and gauge are
  the play's. It is chosen before watching.
- A replay watched is never recorded. A BATTLE play's replay can be watched from its results and kept in a slot, but
  is not kept as the chart's latest or best.

## Practice

E in song select (or the PRACTICE button) opens a chart's practice screen: its notes per measure as a graph, with a
section of measures to play (tap the graph to move its nearer edge).

| setting | values |
|---|---|
| lead-in | 0–4 measures before the section (the music only) |
| speed | 50–200 % in 5 % steps; the sounds are resampled, the judge windows stay in real time |
| gauge, start value | any gauge, its start value |
| `#RANK`, TOTAL | in place of the chart's |
| RANDOM, LN mode | as in play |
| LOOP | the section again at once when it ends or the gauge empties |

- The play starts at the lead-in with what was sounding there. A long note that starts before the section is not
  played; one that starts in it is played whole. The gauge moves by the whole chart's TOTAL and notes.
- In play, R or Backspace starts the section again; Esc goes back to the practice screen, which shows the last try's
  judgements and timing.
- Practice plays are never recorded: no score, lamp, replay, play history or IR. The settings are kept in
  `[practice]`, with the last section per chart.

## G-BATTLE

G-BATTLE plays a 5 or 7 key chart against your own MY BEST, LAST or SLOT 1–4 replay of it. Choose it in song
select's BATTLE row, F1, or DP OPTION in the START panel.

- The chart is made with the replay's seed and lane option on both sides, as LR2 lends a ghost's random. You play the
  first side; the replay's inputs play the second again under its own options (judge, gauge, LN mode, AUTO SCRATCH),
  silently.
- Its EX score is the pacemaker target. If it ever judges otherwise than its replay did (a judge that changed since),
  the play shows GHOST DESYNC and the replay's EX score at your note stands in.
- Your side is saved locally as a normal play (lamp, scores, replays), the opponent's lane option counting for the
  assist level. It is not sent to an IR.
- Replays of BATTLE or LEGACY NOTE plays, of another LN mode, or without inputs cannot be opponents.
- On a phone held upright the single play skin is shown and the opponent is only the pacemaker.
- The results name the winner by EX score; RETRY plays the same battle again.

## VS

VS is two players here, one per side of a 5 or 7 key chart: the BATTLE row's last value, or DP OPTION's.

- Each has their own keys (1P / 2P key config), START / SELECT (2P: `P` / `O` by default), hi-speed, lane cover,
  LIFT, HIDDEN+, gauge, AUTO SCRATCH, judge offset (`[play.p2]` in the config) and lane option (RANDOM / RANDOM 2P).
  Each player's START panel changes their own.
- Each side has its own judge, gauge, score and keysound voices. A player who fails stops and the other plays on; the
  play ends when both have finished or failed. The results show both sides and the winner.
- Nothing is saved: no scores, lamps, replays or play counts.
- Controllers: press a button on the 1P controller while the chart loads; it is the 1P one for this play.
- On one keyboard, many keyboards miss keys when several are held (rollover).
- With touch controls, VS needs a tablet-sized screen (two control sets, each player's START / SELECT on their side);
  phones refuse it.

## IR

An IR (internet ranking) shows rankings and takes scores. Accounts are set in `SETTINGS › ONLINE`: a server speaking
vibeBMS's JSON protocol ([IR_PROTOCOL.md](IR_PROTOCOL.md)), Bokutachi or BMS-IR. Which plays each takes is in RULES
("LR2 equivalence", "Clear lamps").

- An account's API key is kept apart from `config.toml` (on the desktop in `config/credentials.json`), for the server
  it was entered for: after the server's address changes, or in a configuration taken from someone else, the key is
  not sent and `SETTINGS › ONLINE` asks for it again. A server's address is `https://` (`http://` only on this
  computer).
- I in song select (or SCORE STATUS's IR PAGE button) opens the chosen chart's page on the IR the rankings come from,
  in the system's browser (the web build: a new tab). BMS-IR's page is found by the chart's MD5 (a bmson has none);
  Bokutachi's by its own chart id, learnt from your PB lists, so a chart you have no score on there has no page yet;
  a vibeBMS IR's when its server names one.
- **Download my scores** (`SETTINGS › ONLINE`, an account) takes your own scores there into your local records, as
  the score import does: per chart the better lamp, EX score, combo and BP are kept, never a worse value over a
  better one, play counts untouched. The same download again changes nothing. A vibeBMS IR without the route, and
  BMS-IR for anything but lamps, cannot list your scores. How each is filed: RULES "Imported and downloaded scores".

## Score import

`SETTINGS › LIBRARY › Import scores` reads LR2's score database (`LR2files/Database/Score/<player>.db`) or
beatoraja's (`player/<name>/score.db`; LR2oraja's too).

- For the charts in the library: the better lamp, EX score (with its judgements) and max combo, the lower min BP, and
  the play and clear counts as the higher of the two, not summed, so importing again changes nothing.
- Courses and charts not in the library are counted and left out.
- Which judge preset the scores are filed under: RULES "Imported and downloaded scores".

## Streaming

`SETTINGS › SYSTEM › STREAMING` (desktop).

- **Discord Rich Presence** (off by default): your Discord status shows the screen: "Choosing a song" in song select,
  the chart as `title [DIFFICULTY]` on the decide screen, in play ("Playing", "Watching autoplay", "Watching a replay",
  "Practising") and on the result ("Result: HARD CLEAR"), with the time on that screen. It talks only to the Discord
  app on the same computer. It needs a Discord application id (`discord.application_id`): make an application at
  Discord's developer portal, whose name is the game you are shown playing. None is built in. `Discord` below shows
  the state.
- **OBS** (off by default): connects to obs-websocket 5 (OBS 28 or later, `Tools › WebSocket Server Settings`) at
  `obs.host` / `obs.port` (127.0.0.1:4455 by default), with the server's password if it has one. The password is
  kept in `config/credentials.json`, not `config.toml`, for the host and port it was entered for: with another
  address it is asked for again. The connection is not encrypted, so an OBS on another computer is for a network
  you trust. A server that refuses the password is not asked again until the settings change. A scene can be named
  for each screen (song select, decide, play, result); OBS switches to it on
  entering that screen. With `Record plays`, OBS records each play, from its start until its result is left. A slow
  or absent OBS never holds up the game; `OBS` shows the state, and failed requests are logged.

## Skins and sound sets

Skins are chosen in `SETTINGS › SKINS`; a change applies without a restart. What vibeBMS reads of LR2 and beatoraja
skins is in [SKINS.md](SKINS.md).

- A skin's custom options and offsets are on its screen's page in `SETTINGS › SKINS` (Enter on the screen's row).
- `SETTINGS › SKINS › GET SKINS` downloads skins from the catalog ([SKINS.md](SKINS.md), "Skin catalog").
- **Key config**: with a KEY_CONFIG skin chosen, key `6` in song select (or the skin's key config button, or
  `SETTINGS › CONTROLS`) opens its key config screen. ↑↓ or the turntable pick a key, ←→ the mode (7 / 14 keys,
  9 keys, 5 / 10 keys); Enter, a white key or a tap on the key waits for a key, controller button or turntable axis to
  bind; Delete clears it; Esc, SELECT or BACK leaves. It saves as it goes. Without such a skin, `6` opens
  `SETTINGS › CONTROLS`.
- **Skin select**: with a SKIN_SELECT skin chosen, key `7` (or the skin button) opens the skin select screen. ↑↓ pick
  the skin or one of its options, ←→ change it, PgUp / PgDn the skin type, with the skin's preview. Without one, `7`
  opens `SETTINGS › SKINS`.
- Mouse and touch use the skins' buttons; both screens have a BACK button.
- **Sound sets** (`SETTINGS › DISPLAY & AUDIO`, or SOUND SET in the skin select screen): the song select music, the
  decide jingle, folder and option clicks and result fanfares. A set is a folder of beatoraja's sound files or an LR2
  sound set (`.lr2ss`), under `sounds/` or `skins/`. Without one, a few short built-in clicks play (no music). RANDOM
  (with two sets or more) draws a set when the app starts and each time the song select comes back. The files a set
  may hold are in [SKINS.md](SKINS.md), "Sound sets".

## Web build

The web build runs vibeBMS in the browser: [vibebms.github.io/vibe](https://vibebms.github.io/vibe/). Songs are
played from the browser's storage; nothing is uploaded anywhere.

- The page's buttons take a folder, files or song packs (for iPad Safari). Song packs (`.zip`, `.7z`, `.rar`,
  `.lzh`, `.tar`, `.tar.gz`, picked on their own or in a folder) are unpacked as they come in, into a folder of their
  name. An archive picked inside a folder is kept and read as on the desktop; the page reads it into memory to scan
  it or to unpack a song from it (a song of up to 1 GB).
- Songs are kept in the browser's Origin Private File System and are there again the next time the page opens.
  Settings and records are kept in localStorage.
- `SETTINGS › LIBRARY` shows how much room the songs take of what the browser allows, asks the browser to keep them
  for good, lists the song folders by size with a delete for each, and deletes those not played for 30 days or all of
  them. In song select, Delete removes the chosen song's folder. Each delete asks for a second press.
- Add the page to the home screen: it runs full screen, and Safari, which may clear a site's storage after 7 days
  without a visit, keeps it.
- A separate chart added on its own (📦) goes into its song's folder: the folder with the same title and artist that
  holds every sound it needs. When that cannot be decided, the page asks which song it is for.
- The browser paces the frames at the display's refresh rate. Safari keeps pages at 60 frames a second unless
  "Prefer Page Rendering Updates near 60fps" is turned off (macOS: Develop › Feature Flags; iOS and iPadOS: Settings ›
  Apps › Safari › Advanced › Feature Flags).
- Difficulty table servers and the download hosts send no CORS headers, so the web build reads tables and downloads
  archives through the vibeBMS proxy (`proxy.vibebms.workers.dev`), which passes on archives only. Download pages
  open in the system browser.
- IR works when its server allows CORS ([IR_PROTOCOL.md](IR_PROTOCOL.md), "Browsers (CORS)").
- Lua skins work. Skins over 400 MB are not unpacked.
