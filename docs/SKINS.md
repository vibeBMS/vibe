# vibeBMS skins

What vibeBMS reads of LR2 and beatoraja skins, the built-in skins, the skin catalog, sound sets, and the ids vibeBMS
adds for skin authors. Choosing and setting skins is in [MANUAL.md](MANUAL.md), "Skins and sound sets".

Contents: [Built-in skins](#built-in-skins) · [Touch perspective](#touch-perspective) ·
[Files a skin reads](#files-a-skin-reads) · [LR2 skins](#lr2-skins) ·
[beatoraja skins](#beatoraja-skins) · [Battle skins](#battle-skins) ·
[Key config and skin select screens](#key-config-and-skin-select-screens) · [Player totals](#player-totals) ·
[Skin catalog](#skin-catalog) · [Sound sets](#sound-sets) · [vibeBMS extensions](#vibebms-extensions)

## Built-in skins

Landscape and portrait layouts for every mode, with original art drawn from shapes and the Inter font. They are
beatoraja JSON skins in `crates/vibebms/assets/builtin/skins/default/`, made by a script
([BUILDING.md](BUILDING.md), "Development").

- Play skin types 104 / 106 / 108 are 4 / 6 / 8 keys. Only the built-in skins have them, so such charts play on the
  built-in skins (or the touch perspective skin).
- The song select's images are pages of at most 2048×2048: one for the screen and one for each panel, loaded when the
  panel first opens (a `lazy` source, below).
- The song select lets go of its skin's textures while another screen covers it, and loads them again on return.

## Touch perspective

A built-in play skin for every key mode, chosen in `SETTINGS › SKINS` like the others. Each player's lanes are a fan
seen in perspective, narrow at the far end and wider toward the judge line, notes growing as they come.

- **Layout**: the judge line is at three quarters of the screen's height and the lanes reach near its top. At the
  line the keys are about a tenth of the screen wide, and the line between the middle keys (7 keys: 3|4, 5 keys:
  2|3; 2P mirrored) stands upright in the middle of the screen. The scratch, the wider red-tinted lane at the outer
  edge, leans out to its side; its bottom may run off the screen. The panels go in the top corners. Double play shows
  two fans side by side, meeting at the bottom.
- **Speed**: the hi-speed and green number mean what they do on flat lanes. The perspective only changes where a
  distance is on the screen: SUDDEN+, HIDDEN+ and LIFT cover the same share of the lane, which looks smaller at the
  far end.
- **Touch**: the lanes are the controls, in the bottom half of the screen (the judge line in its middle), in zones
  that fill the width like a beatmania controller's keys. At the judge line each key's zone is its lane; between a
  white and a blue key the boundary bends, so the white key is wider below the line and the blue key above it. The
  outer zones reach the screen's edges (in double play, the middle), and the gap beside the scratch is shared by its
  neighbours. Fingers can slide between lanes; the scratch lane takes taps and sideways swipes (a change of
  direction is a new scratch). START / SELECT sit in a top corner. There is no strip of controls: the play screen
  fills the screen.
- **Zones**: not drawn unless the skin's TOUCH AREA option shows them at 25–100 %. The perspective, the lanes' reach
  and the touch area's height are set in `SETTINGS › CONTROLS` (`[perspective]` in the config).
- **JSON**: the skin's canvas takes the screen's shape. Objects carry a vibeBMS `"anchor": [x, y, w, h]`, the share of
  the added width or height they move and grow by. `"perspective": true` makes the game draw the note field as fans.
  A skin's options with ops 960-964 set the touch zones' opacity (0, 25, 50, 75, 100 %); a skin without them draws
  them at the touch controls' opacity.

## Files a skin reads

A skin of any format reads files below `skins/` only, its own theme's or another's: images, fonts and their textures
or pages, `#INCLUDE` / `include`, `#CUSTOMFILE` / `filepath` choices, and what a Lua script opens, loads or asks
`skin_config.get_path` for. `..` and a full path work while they stay below `skins/`; a path that leads out of it
(also a drive or network path) is as a missing file. A sound set's `.lr2ss` likewise finds its sounds below `sounds/`
and `skins/` only.

## Limits

A skin is someone else's file: what it can ask of the game is bounded, far above what skins use.

| | limit |
|---|---|
| a skin file, an included file, a script, a font's glyph list, a file a script reads | 16 MiB; no device or pipe |
| an image, a font, a sound | 256 MiB; no device or pipe |
| files read through `#INCLUDE` / `include` by one load | 16 MiB in all (a file counts 4 KiB at least), 8 levels deep |
| animation frames | 65,536 an object, 1,048,576 a skin |
| gauge cells (`#SRC_GROOVEGAUGE`'s count, `gauge.parts`) | 1000 |
| timing visualizer width (`judgeWidthMillis`) | 2000 ms |
| timing distribution graph bins (`width`) | 2001 |
| a Lua skin: memory | 256 MiB (64 MiB to say what it is when skins are listed) |
| a Lua skin: time | 10 s to load, 2 s to say what it is (type, name, options) when skins are listed, 1 s a frame |
| a Lua skin's table | 64 MiB as JSON, 128 tables deep |
| text a script gives | its first 4096 bytes are shown |

- A skin past a limit of its load (includes, frames, script time or memory) is not loaded: the built-in skin is
  shown and the log says why. So is one whose file makes the reader fail.
- Scripts that run out of a frame's time three frames in a row are stopped for that skin.
- A listing of the skins gives Lua skins 4 s in all to say what they are; those left come with the next listing.
- Choosing a skin loads it once before the choice is saved. A skin the game stopped in the middle of loading (a
  crash, a hang that was ended) is set aside at the next start: its screen has the built-in skin again, and a
  message names it. `cache/skins-loading` is the note of the skins being loaded.

## LR2 skins

`.lr2skin` files under `skins/`; `LR2files\Theme\…` paths are taken from `skins/`.

- Read: images, numbers, text, `#LR2FONT`, gauges, notes, judgements, bar graphs, sliders, buttons, panels, the song
  list, result graphs, `#CUSTOMOPTION` / `#CUSTOMFILE`, DxLib `.dxa` archives.
- `BAR_LABEL`: 0 LN, 1 `#RANDOM`, 2 mines, 3 CN, 4 HCN. The long notes' type is the one the chart gives them, else
  the player's AUTO type, never FORCE. A skin without a CN or HCN label shows its LN one. A course shows what its
  charts have; a course not all installed, nothing.
- `BAR_TROPHY`: 0 bronze, 1 silver, 2 gold, on a table's class course: the last of the table's trophies whose miss
  rate and score rate its best cleared play met.
- `BAR_GRAPH` type 0: a folder's lamp distribution, the best lamps first, 11 source cells an animation frame. Type 1,
  the DJ levels, is not drawn.
- A skin without `#RESOLUTION` (`0`-`3`, or `#RESOLUTION,<width>,<height>`) gets the size its layout reaches: 640x480,
  or 1280x720 / 1920x1080 for HD skins that do not say so.
- Image 101 is the chart's `#BACKBMP` (the stage file when the chart has none).

`#HORIZONTAL` (`#HORIZONTAL,0` is off) makes a play skin scroll right to left:

- A lane's `#DST_NOTE` x, y, w, h is the note at the judge position. The lane runs from there to the right edge of the
  canvas, as tall as the note, and notes come in from that edge, keeping their images' orientation.
- Long notes have a horizontal body between their two ends. Bar lines (`#DST_LINE`'s x is the judge position) and the
  BPM guide move the same way.
- LIFT moves the judge position right, SUDDEN+ covers the right end and HIDDEN+ the left. The LIFT, lane cover and
  hidden cover offsets (3-5) move objects along x; `#DST_NOTE2` is the x missed notes go to. A cover's disappear
  line still crops vertically.

## beatoraja skins

JSON (`.json`) and Lua (`.luaskin`: embedded Lua 5.4 in a sandbox, in the web build too).

- `main_state`, `timer_util`, `event_util` and `skin_config` are provided. Conditions, timers, values and events
  given in Lua are evaluated every frame.
- `main_state.event_exec` (a game event: an option changed, a play started, a page opened) is taken while the script
  of a clicked object runs, 8 a click, and is `false` anywhere else. A `customEvents` action is a script; one that
  is an event's id is not run.
- Lua is 5.4 with the libraries a skin needs: no `debug`, no files but the skins folder's (read only), no network.
  Tables have no finalizers (`__gc`), `collectgarbage` only counts, `load` takes text.
- Graphs (`gaugegraph`, `judgegraph`, `bpmgraph`, `timingdistributiongraph`, the timing visualizer) and blend modes
  are supported.
- Text outline and shadow are drawn as beatoraja draws them: TrueType and plain `.fnt` fonts get the shadow only (the
  text in half its colour at the offset); distance-field `.fnt` fonts (font `type` 1 / 2) the outline, shadow colour
  and softness too.

## Battle skins

Types 12 / 13 ("7KEYS BATTLE" / "5KEYS BATTLE" in `SETTINGS › SKINS`; a 14 / 10 key skin can be chosen there too) are
drawn when both sides of a battle are shown (G-BATTLE, VS). The built-in ones have a gauge per side and the opponent
as the target.

| id | when there is an opponent |
|---|---|
| LR2 `#SRC_GROOVEGAUGE` with 1 in its first field; JSON gauge destination `"side": 1` | the second side's gauge |
| numbers 120-136 | its values as LR2's 2P: score, EX, rate, combo, max combo, notes, gauge, next rank, PG..POOR, total rate |
| number 108 | 1P minus 2P |
| numbers 121 / 151 / 128 / 153, bar graphs 114 / 115 | its EX score as the target |
| timers 43 / 45 / 49 / 144 | its gauge rising, full, full combo, last note (42 / 143 the first side's) |
| options 44 / 45 | its groove / survival gauge |
| options 56 / 57 | its AUTO SCRATCH |
| options 210-217 | its DJ level |
| options 352 / 353 / 354 | the first side winning / losing / level |
| result: numbers 120-136, options 352-354 | likewise |
| result: option 9350 | a battle |
| result: text 9351 | the opponent's name |
| result: numbers 9376 / 9370 | its miss count and lamp |

A skin's 2P judge region shows the opponent's judgements. A single play skin shown over a battle (a phone held
upright) never does.

## Key config and skin select screens

Skin types 8, 9 and 10 are the key config, skin select and sound set screens.

- Key config: beatoraja's rows are drawn over beatoraja skins as beatoraja does. LR2 skins use their key buttons
  (101-136, 140-143, slots 150-159; Lunatic Vibes' texts 300-384 too).
- Skin select: the preview is beatoraja's `skinpreview`, or LR2 image 105.

## Player totals

Numbers 17-19, 30-37 and 333: plays and clears are summed over the charts' records; judgements and time played are
counted after every recorded play (not autoplay or replays). For plays older than the totals: RULES.md, "Older
records".

## Skin catalog

vibeBMS bundles no third-party skins. `SETTINGS › SKINS › GET SKINS` lists the skins in
[`skins/catalog.json`](../skins/catalog.json) (read from this repository, the copy compiled in until then) and
downloads, unpacks and installs one into `skins/` from the link it lists (Google Drive, Dropbox, MEGA, GitHub, plain
servers), replaces it with a newer version, or deletes it.

Each entry names its author, scenes, resolution, homepage, the terms its author states, and where its download comes
from (`source`):

| `source` | meaning |
|---|---|
| `official` | the skin's terms state that it may be freely used and redistributed (a licence such as MIT, Apache-2.0 or GPL, or the author's own words), and the download is the author's own link |
| `third_party` | every other one: terms that say nothing of it or forbid redistribution (even from the author's own link), a re-upload, an archive of a dead site, a pack of several authors' skins |

- Third-party entries are listed and downloaded only with third-party sources on (`SETTINGS › LIBRARY`, the same
  choice and notice as for songs). While it is off, GET SKINS says how many it leaves out, and a skin of theirs
  already installed stays listed, to be deleted.
- `official` is this build's word: in a catalog read from the repository later, an entry is `official` only if the
  catalog compiled in lists it so with the same `downloads` (and `sha256`). Any other is offered as `third_party`.
- `downloads` are `https://` links; an entry with another kind of link is left out.
- `sha256` (optional) lists the SHA-256 of each archive in `downloads`, in the same order (hexadecimal; `""` for one
  not listed). A download that is not the listed file is not installed.
- An install puts its files in the entry's `folder` (an `LR2files/Theme/<name>/` archive: in its themes' folders).
  It replaces the folders the entry has (its `folder`, those its last install left) and makes new ones; an archive
  that would take the place of any other folder of `skins/` is refused.
- An entry without `source` is `official` in a catalog of version 1, else `third_party`. A value vibeBMS does not know
  is `third_party`.
- The web build keeps skins in the browser's storage, but does not unpack skins over 400 MB.

## Sound sets

A sound set is the song select music, the decide jingle, folder and option clicks and result fanfares. It is a folder
of beatoraja's sound files (`select.wav`, `decide.wav`, `f-open.wav`, `o-change.wav`, `clear.wav`, ...; any audio
format) or an LR2 sound set (`.lr2ss`), under `sounds/` or `skins/`.

- An `.lr2ss` names its sounds by LR2's paths (`LR2files\Bgm\…`): they are looked for beside the folder holding the
  set, in `sounds/` and beside the set ([Files a skin reads](#files-a-skin-reads)).
- A screen's music (the select music, the result's) is decoded each time it starts and let go when it stops; the
  other sounds stay decoded.
- The select music, the decide jingle and `.loop` music are normalised to the loudness target when
  `Normalize sound set music` is on; the other sounds are not.
- GUIDE SE files: [MANUAL.md](MANUAL.md), "Play extras".

A folder set may also use bmz-player's conventions, as its documentation describes them:

- **Result music by outcome**: `fail` for a failed play; for a clear, `aaa`, `aa` or `a` at that rank (the play's EX
  score from 8/9, 7/9, 6/9 of what can be had), else `clear`. A missing rank sound falls back on `clear`, not on a
  lower rank. A course's result keeps `course_clear` / `course_fail`.
- **`<name>.loop.<ext>`** (`select.loop.ogg`, `clear.loop.ogg`, `aaa.loop.wav`, ...): plays over and over until the
  screen ends, before `<name>.<ext>` in the same folder.
- **`<name>.enter.<ext>`** (`select.enter`, `clear.enter`, `aaa.enter`, ...): plays once as the screen comes (the song
  select: at start and each time it comes back), with the screen's music. A result with one plays a `.loop` music
  with it, not a plain one.
- **Variants**: the set's subfolders holding sound files are its variants, not sets of their own. One is drawn with
  the set and has only what it changes: a sound it has in either form (its `clear` before the set's `clear.loop`) is
  its own, else the set's; `.enter` and GUIDE SE files likewise.
- **RANDOM** (with two sets or more): a set drawn at random when the app starts and each time the song select comes
  back; a set with variants draws one again then too. Decide, play and result keep the set drawn.

## vibeBMS extensions

Ids vibeBMS adds to the ones beatoraja and LR2 skins use:

| kind | id | meaning |
|---|---|---|
| song select option text | `9000+i` (value), `9100+i` (name) | the select panel's option `i` |
| song select click event | `9000+i` | change the select panel's option `i` |
| song select options | 45, 46, 47 (text and event 9045-9047) | the song's folder (F3), the chart's web page (U), SAME FOLDER (B) |
| song select events | 9900, 9901, 9902, 9903, 9905, 9908 | parent folder, watch the chart's replay, read its text files, search, practice the chart, its IR page |
| select event | 9920 | open the profiles (`SETTINGS › SYSTEM › PROFILE`) |
| result events | 9500, 9501, 9502 | retry, continue, watch the replay |
| button | 1407 | the key mode filter with 4 / 6 / 8 KEYS (indices 6-8); button 11 shows them as ALL, as bmz-player's |
| buttons | 10 / 11 / 12 | the difficulty filter, key mode filter and sort |
| text | 9218 | the profile's name |
| text | 9200 | the chosen chart's IR line: the lamp beside yours and the rank; a local rival's lamp and EX score |
| text | 1 / 3 | the rival's name / the target's name; the target's EX score as the target's numbers |
| number | 9425 | the record's min CB (fewest combo breaks); 425 is the play's |
| options | 1242 / 1243 | FAST / SLOW, shown past the FAST / SLOW threshold |
| result option, text, numbers | 9350, 9351, 9376 / 9370 | battles (above) |
| options (ops) | 960-964 | touch zone opacity (Touch perspective) |
| play skin types | 104 / 106 / 108 | 4 / 6 / 8 keys (built-in skins only) |
| timers | 21 / 31 | START / SELECT during play: panel open / closed |
| slider / graph value | 9400 | the hi-speed position (0.5–10.0) |

- For skins with no object using timer 21, the app draws a hi-speed panel beside the lanes.
- A graph with `"style": "vibebms"` is drawn as the built-in skins draw it.
- A beatoraja `source` with `"lazy": true` is loaded when something is first drawn from it (the built-in song
  select's panels). Until it is loaded, the objects that share an option with one drawn from it wait too, so a panel
  appears whole a frame or two later; clicks on them still count.
