# Judge and gauge rules

All constants live in `crates/vibebms-core/src/judge.rs` and `gauge.rs`.
Timing difference is `note_time - press_time` (positive = early / FAST).
Windows are written as `late .. early` in milliseconds.

## Judge presets

### beatoraja (default)

Values from beatoraja `JudgeProperty` (#RANK EASY = 100 %).

| mode | PGREAT | GREAT | GOOD | BAD | empty POOR |
|---|---|---|---|---|---|
| 5/10 keys | ±20 | ±50 | ±100 | ±150 | -150 .. +500 |
| 5/10 keys scratch | ±30 | ±60 | ±110 | ±160 | -160 .. +500 |
| 7/14 keys | ±20 | ±60 | ±150 | -280 .. +220 | -150 .. +500 |
| 7/14 keys scratch | ±30 | ±70 | ±160 | -290 .. +230 | -160 .. +500 |
| 9 keys (PMS) | ±20 | ±50 | ±117 | ±183 | -175 .. +500 |

LN / CN release windows (7 keys): ±120 / ±160 / ±200 / -280..+220 (scratch ±130 / ±170 / ±210 / -290..+230).
PMS has a 200 ms LN release margin.

`#RANK` scaling (`JudgeWindowRule`): VERY HARD 25 %, HARD 50 %, NORMAL 75 %, EASY 100 %, VERY EASY 125 %
applied to PG..BAD (empty POOR fixed). `#DEFEXRANK n` = n % of NORMAL. bmson `judge_rank` is used directly.
For PMS, PG and BAD are fixed and GR/GD scale 33/50/70/100/133 %, clamped between PG and BAD.
Custom judge (PG/GR/GD %) is applied afterwards and clamped to BAD.

Other rules: empty POOR does not consume the note; in 7 keys it keeps the combo, in 5 keys/PMS it breaks it;
PMS notes can only produce one POOR. Note selection algorithms Combo / Duration / Lowest as in beatoraja.

### LR2

Values as LR2oraja (Endless Dream) reproduces LR2.

| #RANK | PGREAT | GREAT | GOOD | BAD |
|---|---|---|---|---|
| VERY HARD | ±8 | ±24 | ±40 | ±200 |
| HARD | ±15 | ±30 | ±60 | ±200 |
| NORMAL | ±18 | ±40 | ±100 | ±200 |
| EASY | ±21 | ±60 | ±120 | ±200 |

The rows are LR2's measured windows [S5, S2, S3, S4]. A chart without `#RANK` is NORMAL [S8, S9]. `#RANK 4` (VERY
EASY) and values out of range are NORMAL here; what LR2 does with them is not documented (gap A3). LR2 has no
`#DEFEXRANK`: vibeBMS takes it as a percentage of NORMAL, the judge rank then lies between the rows above (NORMAL =
75 %, EASY = 100 %) and the windows are interpolated between them; above EASY they grow in proportion (bmson
`judge_rank` is taken as that percentage). This is vibeBMS's own rule. Windows stay inside BAD.

Empty POOR: up to 1000 ms early, never late [S1, S2, S3, S4]. In every key mode, as LR2 [S4, S8]: BAD and POOR
break the combo, an empty POOR keeps it; a BAD takes the note (so a second press can BAD the next one), an empty
POOR never does, so one note can give any number of them.

LN / CN releases: PG, GR and GD share the #RANK's GOOD window (±40 / ±60 / ±100 / ±120 ms), BAD ±200 ms. An LN
keeps its head judgement when released inside the GOOD window, and is a BAD when released earlier [S4, S7]. LR2
itself has only such LNs: no CN, no HCN, no tail judgement of its own [S8].

These windows are LR2's with vertical sync off; with it on, LR2 judges on frames and they change [S4]. vibeBMS
judges in continuous time.

### IIDX (approximation)

Counted in frames of the 120 Hz LIGHTNING MODEL cabinet: PG ±2F, GR ±4F, GD ±14F, BAD ±30F
(±16.67 / 33.33 / 116.67 / 250 ms), independent of #RANK. Input timing is quantised to the 120 Hz frame grid.
**These are community-known approximations, not measured arcade values.**

## Gauges

Values are change per PG, GR, GD, BAD, POOR, empty POOR (percent).

### beatoraja 7 keys (`GaugeProperty.SEVENKEYS`)

| gauge | init | border | values | modifier |
|---|---|---|---|---|
| ASSIST EASY | 20 | 60 | 1, 1, 0.5, -1.5, -3, -0.5 | × TOTAL / notes on gains |
| EASY | 20 | 80 | 1, 1, 0.5, -1.5, -4.5, -1 | × TOTAL / notes |
| NORMAL | 20 | 80 | 1, 1, 0.5, -3, -6, -2 | × TOTAL / notes |
| HARD | 100 | – | 0.15, 0.12, 0.03, -5, -10, -5 | gain limit; damage ×0.4..0.8 below 10..50 % |
| EX-HARD | 100 | – | 0.15, 0.06, 0, -8, -16, -8 | gain limit |
| HAZARD | 100 | – | 0.15, 0.06, 0, -100, -100, -10 | |
| CLASS | 100 | – | 0.15, 0.12, 0.06, -1.5, -3, -1.5 | damage ×0.4..0.8 below 5..25 % |
| EX-CLASS | 100 | – | 0.15, 0.12, 0.03, -3, -6, -3 | |
| EXHARD-CLASS | 100 | – | 0.15, 0.06, 0, -5, -10, -5 | |

The 5-key and PMS sets are in `gauge.rs` (also from beatoraja's `GaugeProperty`).
Default TOTAL when missing: `max(260, 7.605·n / (0.01·n + 6.5))` (n = judgements: CN/HCN count twice);
with the LR2 gauges `160 + (n + clamp(n − 400, 0, 200)) × 0.16`. A bmson `total` is a percentage of that default.

**5-key EX-HARD damage modifier** (beatoraja's `MODIFY_DAMAGE`): damage × max(TOTAL factor, note-count factor).
beatoraja's note-count loop also adds negative terms for charts with more notes than the current step, which
effectively disables it; vibeBMS keeps only the positive part.

### LR2 set (LR2oraja Endless Dream's `GaugeProperty.LR2`)

| gauge | init | border | values | modifier |
|---|---|---|---|---|
| ASSIST EASY | 20 | 60 | 1.2, 1.2, 0.6, -3.2, -4.8, -1.6 | × TOTAL / notes on gains |
| EASY | 20 | 80 | 1.2, 1.2, 0.6, -3.2, -4.8, -1.6 | × TOTAL / notes |
| NORMAL | 20 | 80 | 1, 1, 0.5, -4, -6, -2 | × TOTAL / notes |
| HARD | 100 | – | 0.1, 0.1, 0.05, -6, -10, -2 | LR2 damage; ×0.6 below 32 % |
| EX-HARD | 100 | – | 0.1, 0.1, 0.05, -12, -20, -2 | LR2 damage |
| HAZARD | 100 | – | 0.15, 0.06, 0, -100, -100, 0 | LR2's DEATH: an empty POOR costs nothing [S8] |
| CLASS | 100 | – | 0.1, 0.1, 0.05, -2, -3, -2 | ×0.6 below 32 % |
| EX-CLASS | 100 | – | 0.1, 0.1, 0.05, -6, -10, -2 | ×0.6 below 32 % |
| EXHARD-CLASS | 100 | – | 0.1, 0.1, 0.05, -12, -20, -2 | |

The survival gauges (HARD to EXHARD-CLASS) empty when they fall below 2 %. LR2 damage: damage × the larger of a
TOTAL factor and a note-count factor. The TOTAL factor is S1's measured POOR damage over 10 at the multiple of 10
at or below TOTAL, LR2 dropping the ones digit [S1]: 1 from 240, 1.11 from 230, 1.25 from 210, 1.5 from 200, 1.666
from 180, 2 from 160, 2.5 from 150, 3.333 from 130, 5 from 120, 10 below (the table S2 and S4 give). The note-count
factor rises from 1 at 1000 notes to 10 at 20 (1 + 0.002·(1000 − n) below 1000, 2 + 0.004·(500 − n) below 500, 3 +
0.008·(250 − n) below 250, …).

EASY, NORMAL (GROOVE), HARD, HAZARD and CLASS (dan) are LR2's gauges; their values, the 32 % rule, the 2 % floor and
the HARD damage factor from 125 notes up are as LR2 is documented [S1, S3, S7, S8]; below 125 notes the note-count
factor is vibeBMS's guess (gap A4). ASSIST EASY, EX-HARD, EX-CLASS and EXHARD-CLASS are not LR2's [S8]: the set
offers them, but the vibe-LR2 profile does not take them (gap A10).

### IIDX set (approximation)
Groove gauges start at 22 %, HARD ±0.16 gain / -5, -9, -5 damage halving below 30 %, EX-HARD -10, -18, -10.
**Approximate values**, adjust in `gauge.rs`.

## Long notes
* **LN** (`#LNMODE 1`, default): only the start is judged; the final judgement is the worse of the start and
  the release (release windows above). Releasing earlier than the BAD release window gives BAD.
* **CN** (`#LNMODE 2`): start and end are judged separately (counts as 2 notes). Holding more than the BAD
  late window past the end gives POOR.
* **HCN** (`#LNMODE 3`): CN plus a gauge tick every 200 ms of the body: +GREAT×0.5 while held, BAD×0.5 while released.
* Back-spin scratch: on a scratch CN/HCN the end is judged when the turntable is spun the other way. For
  keyboards and touch, releasing also ends it (configurable in the engine).
* Scores, replays and IR entries are filed by LN mode: a chart without long notes under LN; a chart whose long
  notes all have their type (`#LNMODE`, bmson `t`) under that type (with several types, the strongest:
  HCN > CN > LN), whatever the player's LN mode; otherwise under the player's LN mode. Records such charts had
  under the player's mode are merged into the chart's when the library loads.

## Clear lamps
FAILED < ASSIST (assist options) < L-ASSIST (light assist options, ASSIST EASY gauge) < EASY < CLEAR < HARD <
EX-HARD < FULL COMBO (no BAD/POOR; HAZARD survival) < PERFECT (no GOOD) < MAX (all PGREAT). DJ level thresholds
at n/9 of max EX.

Assists (as beatoraja's `BMSPlayer`; an option counts only when it took effect, `AssistFacts` in `score.rs`):
* ASSIST: AUTO SCRATCH on a chart with scratch notes, LEGACY NOTE on a chart with long notes, EXPAND JUDGE over 100 %.
* LIGHT ASSIST: SPIRAL, H-RANDOM, ALL-SCR, RANDOM+, S-RANDOM+, BATTLE, NO MINE that removed mines, BPM GUIDE on a
  chart that changes BPM.

An assisted play that is not FAILED gets that lamp whatever the gauge. It keeps only the lamp, the play and clear
counts and the player's totals: no EX score, BP or combo best, no best replay (MY BEST ghost), nothing sent to IR.

## LR2 equivalence (the vibe-LR2 profile)

The vibe-LR2 rule profile (`crates/vibebms-ir/src/rule.rs`) is sent to a third-party IR as LR2 rules only for
plays no open gap of its equivalence list reaches. A gap closes when public documentation of LR2's behaviour
establishes the rule and the engine follows it under the LR2 preset (or the profile leaves the case out); a gap is
narrowed when its reach is cut down to the plays it can change, which are then refused with the gap's reason.
Nothing here comes from the source of beatoraja, LR2oraja or bmz-player, or from LR2's binary.
`tools/lr2-measure/` holds test charts and steps for a player with LR2 to settle the open gaps.

| gap | what | state |
|---|---|---|
| A1 | judge and gauge presets were separate settings | closed: the profile takes only the LR2 judge with the LR2 gauge set |
| A2 | empty POOR window and its combo and note rules | closed: 0 .. +1000 ms, early only [S1, S2, S3, S4]; keeps the combo, never takes the note [S4] |
| A3 | `#RANK 4`, out-of-range `#RANK`, `#DEFEXRANK`, bmson | narrowed to charts whose `#RANK` is not 0 .. 3: `#RANK 0..3` and a missing `#RANK` are LR2's [S5, S8, S9]; LR2 has no `#DEFEXRANK` [S9] or bmson, and the profile leaves both out. `#RANK 4`: LR2 does not play it VERY EASY [S9 does not list LR2 among its players, S16], and "it appears" LR2 plays it NORMAL [S13], as vibeBMS does; one hedged source, so still refused. Other values: undocumented (vibeBMS: NORMAL). A `#RANK` vibeBMS cannot read as a number plays NORMAL and is not refused (LR2's handling unknown; no such chart is known) |
| A4 | HARD gauge damage factor and the 30 % rule | narrowed to HARD on charts of 1 .. 124 notes and TOTAL 120 or more. Confirmed: the base values, BAD = POOR × 0.6, empty POOR = POOR × 0.2, the larger of the TOTAL and note-count factors, × 0.6 below 32 %, empty below 2 % [S1, S3, S7]; the TOTAL factor at every TOTAL: S1 measured each multiple of 10 from 100 to 300 (200 included: 15) and says LR2 drops the ones digit, so a TOTAL between takes the measured value below it [S1]; vibeBMS now does (it stepped every 16 and took 14.29 at 200). The note-count factor from 125 notes up: S1's slopes to 250, then its graph's mark at 125 with the slope doubled again as S1's "…" says [S1]. Not confirmed: the note-count factor below 125 notes, where S1 gives only the graph (not readable: the archived image does not load) and its marks 40 at 125 and 100 at 0; it can decide only when the TOTAL factor is below 10 (TOTAL 120 or more). HAZARD is LR2's DEATH gauge [S8] |
| A5 | LN mode and typed CN / HCN | closed: LR2 has only LNs without a tail judgement [S8]; the profile takes LN mode and no typed CN / HCN; a release inside GOOD keeps the head's judgement, earlier is a BAD [S4, S7] |
| A6 | combo, note consumption and one-POOR rules | closed: LR2's in every key mode [S4, S8] (5/10 keys and PMS had beatoraja's) |
| A7 | default TOTAL | narrowed to charts without `#TOTAL` (or with one of 0 or less), on every gauge but HAZARD: vibeBMS takes 160 + (n + clamp(n − 400, 0, 200)) × 0.16, which LR2oraja's README says it changed to "to match that of LR2", with a graph labelled "LR2 / LR2oraja" [S13, S11]; S9 gives LR2's as "160?", which the formula is at n = 0. Neither says how LR2's was established |
| A8 | mines, scratch long note ends | narrowed to charts with mines: vibeBMS takes a mine's value as percent of the gauge, as S14 gives for LR2 (`ZZ`: FAILED on survival gauges, 2 % on EASY and GROOVE; the same damage on every gauge; a mine goes off held through or within the GOOD window); S9 and S15 say value / 2 %, which is nanasi's (S15 is its column); S10 measured on GROOVE −8, −8, −10, −12, −16 % for 1, 2, 3, 5, 10 % and an end on survival gauges. S10's five readings fit "the value as percent plus 6 %" (a POOR's damage) to the 2 % steps LR2 shows, but nothing says a POOR comes with a mine. vibeBMS sets a mine off only when its key is down as the mine passes; S14 also has a press within the GOOD window set it off. Scratch LN ends: LN mode only under the profile, judged as keys' [S4, S7]. GAUGE AUTO SHIFT: the profile takes none |
| A9 | origin of the LR2 golden vectors | closed: recorded in `judge_golden.rs` and `gauge_golden.rs`; the documented behaviour has its own tests (`lr2_*_as_measured`, `lr2_hard_low_gauge_rule`, `lr2_combo_and_empty_poor_in_every_mode`, `windows_lr2`, `s14_lr2_release_window`) |
| A10 | gauges LR2 does not have | closed: the LR2 set offers ASSIST EASY and EX-HARD (and EX-CLASS, EXHARD-CLASS in courses), which LR2 has not [S8]; the set keeps them, but the profile takes only EASY, NORMAL, HARD, HAZARD and CLASS, so a play on the others is local rules |
| A11 | the note a press takes among several in reach | open, every play: the profile plays vibeBMS's Combo choice. S4 says beatoraja's "combo" choice is "reportedly" close to LR2's, and beatoraja 0.3.7 named that choice "LR2風" (LR2 style: notes in the GOOD window first) [S17]; neither measured LR2. S4's description of the combo choice (the note a GOOD or better can take; when both or neither can, the lower one) differs from vibeBMS's Combo in one case: when both notes are late beyond GOOD, vibeBMS takes the later one. `tools/lr2-measure/` tells the candidates apart |

One more difference no gap covers, because it is not a rule: LR2 judges on frames, so its windows are those
above only with vertical sync off [S4].

Bokutachi takes LR2 and LR2oraja plays into its BMS 7K and 14K rankings [S18]: it treats LR2oraja's rules (LR2's
windows and gauges in a beatoraja, with beatoraja's note choices) as LR2's. What it asks of the note choice is
not written down; that is the question for its maintainer.

The beatoraja profile's gaps stay open: no public description was found of beatoraja's 5-key EX-HARD note-count
term (B1) or of how beatoraja ends a back-spin scratch CN / HCN played on a keyboard (B3), and beatoraja's source
is not read. (S4's description of beatoraja's combo choice, above under A11, also differs from vibeBMS's Combo when
both notes are late beyond GOOD; it is not on the list, being one reading of one source.)

### Sources

Read for this list (2026-11). "Measured" means the author timed LR2 itself.

- **S1** yoshi_65c816, "LR2の仕様まとめ" (LR2 rules summary), archived
  <https://web.archive.org/web/20190210133959/http://2nd.geocities.jp/yoshi_65c816/bms/LR2.html>. Measured. Empty
  POOR only on early presses; no POOR from presses after a note is hit. Gauge per PG, GR, GD, POOR, BAD, empty POOR
  (t = TOTAL, n = notes): GROOVE t/n, t/n, t/2n, −6, −4, −2; EASY ×1.2 gains, −4.8, −3.2, −1.6; HARD 0.1, 0.1, 0.05,
  −10, −6, −2; dan 0.1, 0.1, 0.05?, −3, −2, −2. The gauge is kept with decimals and shown in 2 % steps. HARD damage
  grows for low TOTAL or few notes (measured with 1000 notes or more): POOR damage at TOTAL ≥ 240: 10, 230: 11.1,
  220 and 210: 12.5, 200: 15, 190 and 180: 16.66, 170 and 160: 20, 150: 25, 140 and 130: 33.33, 120: 50, ≤ 110: 100.
  "10刻みで設定されている？1の位以下は切り捨て" (set in steps of 10?; the ones digit and below are dropped); it
  names a script that sets TOTAL 159 to get 150's damage. Below 1000 notes POOR damage grows 0.02 % a note down to
  500 and 0.04 % a note down to 250, "…という補正" (and so on), then as a graph (`file/tn_graph.png`; its marks: 125
  notes 40, 0 notes 100; the archived image would not load for this pass). The larger of the two is taken; BAD is
  POOR × 0.6, empty POOR POOR × 0.2. At 30 % (as shown) or less, HARD damage × 0.6. Re-read 2026-11 from the
  archived page itself (captures of 2013-09 and 2019-02 agree).
- **S2** 園田うき, "beatoraja vs Lunatic Rave2(LR2)", 2019,
  <https://www.ukitouchtypist.org/2019/09/17/beatoraja-vs-lunatic-rave2/>. LR2 EASY ±21 / ±60 / ±120 / ±200, empty POOR
  +1000; NORMAL ±18 / ±40 / ±100 / ±200, +1000. EASY and HARD gauges as S1; HARD × 0.6 at 30 % and below. Its LR2
  TOTAL table (≥ 240 1.0, ≥ 230 1.11, ≥ 210 1.25, ≥ 200 1.5, ≥ 180 1.666, ≥ 160 2.0, ≥ 150 2.5, ≥ 130 3.333, ≥ 120 5.0,
  else 10.0) cites beatoraja's emulation, not LR2.
- **S3** iidx.org, "IIDX LR2 beatoraja differences", <https://iidx.org/misc/iidx_lr2_beatoraja_diff>. LR2 windows for
  all four ranks (as the table above), empty POOR +1000, "only possible before a note; not after". Gauges as S1; LR2
  HARD at or below 30 %: BAD −3.6, POOR −6, empty POOR −1.2; dan at or below 30 %: −1.2, −1.8, −1.2. Notes that
  LR2oraja says 32 % and death at 2 %, "not confirmed by other sources".
- **S4** ralba_gear, "【発狂PMS】beatoraja/LR2仕様比較まとめ", 2023 (updated 2024-02),
  <https://ralba-gear.hateblo.jp/entry/2023/11/06/141139>. LR2 windows for all four ranks, empty POOR +1000 .. 0 ms,
  measured with vertical sync off. LR2 LN: the end window is the GOOD window; released inside it, the LN gets the
  head's judgement; released earlier, an early BAD. In LR2 an early BAD takes the note (so the next press can BAD
  the next note); an empty POOR can be had any number of times per note; an empty POOR keeps the combo. beatoraja's
  "combo" note choice is "reportedly" close to LR2's. LR2 gauges as S1; the TOTAL table of S2, again from beatoraja.
- **S5** hitkey, diary 2015-01-19 "Memo: LR2 timegates",
  <https://web.archive.org/web/20221202153721/https://hitkey.nekokan.dyndns.info/diary1501.php#D150119>. Measured by
  803832 and Bluvel: PG 8 / 15 / 18 / 21, GR 24 / 30 / 40 / 60, GD 40 / 60 / 100 / 120, BAD 200 ms (#RANK 0..3).
- **S6** lntakeshi, "beatorajaの設定などのまとめ", 2017, <https://lntakeshi.hateblo.jp/entry/2017/05/19/002127>. No
  late empty POOR in LR2; LR2 gauges as S1; HARD × 0.6 from 30 %.
- **S7** LR2oraja 0.8.3 release notes (prose), <https://github.com/wcko87/lr2oraja/releases/tag/build1099684698>. "HARD
  Gauge damage reduction now starts when you have <32% gauge, like in LR2"; on HARD you die below 2 %; "The LN
  premature release window in LR2 is the same as the GOOD window of a regular note"; a premature release is a BAD.
  It does not say how this was established.
- **S8** Hakula, "LR2 配置教程 - Select", <https://hakula.xyz/posts/tutorial/lr2/select/>. LR2's gauges: GROOVE
  (20 % start, 80 % to clear, never below 2 %), EASY, HARD (SURVIVAL), HAZARD (DEATH: a BAD or POOR ends the play, an
  empty POOR costs nothing), P-ATTACK, G-ATTACK; "LR2 has no ASSISTED-EASY and EX-HARD". A chart without a judge
  rank plays NORMAL. A full combo in any gauge is a FULL COMBO. LR2 cannot change the LN type; LNs have no tail
  judgement.
- **S9** hitkey, "BMS command memo", <https://hitkey.nekokan.dyndns.info/cmds.htm> (read in full 2026-11): LR2
  applies `#RANK 2` when `#RANK` is missing; LR2 is not among the players listed for `#RANK 4` (nanasi, fgt++,
  iBMSC, Angolmois, Sonorous, TechnicalGroove) or `#DEFEXRANK`; LR2's default `#TOTAL` is given as "`160`? (Note:
  NORMAL Gauge)"; landmines (`#xxxD1-E9`, LR2 among their players) take value / 2 % of the gauge and `ZZ` fails the
  stage (not said to be measured in LR2); "LR2 cannot process duplication of land-mine channels well".
- **S10** Obj Tech Lovers, "Guidance chapter 4-7",
  <https://nekokan.dyndns.info/~otlovers/guidance/guidance_4b.html>. Measured in LR2 from a 20 % GROOVE gauge, one
  mine: damage 1 % → −8 %, 2 % → −8 %, 3 % → −10 %, 5 % → −12 %, 10 % → −16 % ("the flow is a mystery"); on survival
  gauges a mine seems to end the play whatever its damage; `ZZ` takes the gauge to 2 %.
- **S11** terrastellal, "BMSの難易度に関する話", 2022, <https://terrastellal.hatenablog.com/entry/HappyCentury>. Says
  the default-TOTAL formula vibeBMS uses for LR2, 160 + (n + clamp(n − 400, 0, 200)) × 0.16, is LR2oraja's, and that
  the LR2 TOTAL table of S2 is beatoraja's. (The article quotes a few lines of beatoraja and LR2oraja code; nothing
  was taken from them.)
- **S12** Tachi, `docs/src/game-support/games/bms-7K.md`,
  <https://github.com/zkldi/Tachi/blob/main/docs/src/game-support/games/bms-7K.md>. BMS 7K lamps: NO PLAY, FAILED,
  ASSIST CLEAR, EASY CLEAR, CLEAR, HARD CLEAR, EX HARD CLEAR, FULL COMBO; judgements pgreat .. poor. It states no rule
  set.
- **S13** wcko87, LR2oraja README, <https://github.com/wcko87/lr2oraja> (README prose only), and its linked graph
  "beatoraja vs LR2 default #TOTAL calculation", <https://imgur.com/a/X9PnxsP>. "VERYEASY judge in LR2oraja is set to
  75%, the same as NORMAL judge. It appears that LR2 treats VERYEASY judge as NORMAL judge." "The default TOTAL
  calculation formula has been changed to match that of LR2": `160 + (N + clamp(N-400, 0, 200))*0.16`; the graph
  labels that line "LR2 / LR2oraja #TOTAL" (gradient 0.16, doubled between 400 and 600 notes). How LR2's was
  established is not said.
- **S14** aigue, "地雷オブジェに関するアレコレ" (all about mine objects), <http://losak.web.fc2.com/jirai.html>.
  "LR2における地雷オブジェの具体的な仕様": channels D1-D9 / E1-E9, `#WAV00` the explosion; damage (%) = the value
  (decimal), e.g. `0U` = 30 %; `ZZ` forces FAILED whatever the gauge, but "即死ダメージはEASYかGROOVEなら2%になるだけ"
  (on EASY or GROOVE it only takes the gauge to 2 %); no difference in damage between EASY, GROOVE, SURVIVAL,
  P-ATTACK, G-ATTACK and DEATH; a mine goes off when its key is on and it is within the GOOD window, or the key is
  held as it passes; mines do not count in TOTAL NOTES. Credits S15 for its explanation. Not said to be measured.
- **S15** Toy Musical, column 3 "踏むと爆発するアレ", <https://tm1.toymusical.net/column/3.html> (a nanasi-groove
  game's column): the same spec as S14 but damage (%) = value / 2 (`0K` = 10 %).
- **S16** a comment on PABAT! 2021, "ROADBLOCKER - Deadline Hater",
  <https://k-bms.com/party_pabat/party.jsp?board_num=21&num=2&order=reg&odtype=a>: "#RANK 4 (Very Easy) doesn't work
  correctly on LR2. Use #RANK 3 instead."
- **S17** Ryuya, "LR2の後継機(?)beatorajaの起動・設定・実際にプレイするまでの手順", 2017 (beatoraja 0.3.7),
  <https://5pmeternal.hatenablog.com/entry/2017/02/15/150245>: beatoraja's judge algorithm options were "LR2風
  (GOOD範囲内のノーツを優先、コンボが切れにくい)" (LR2 style: notes in the GOOD window first), "IIDX風
  (最も近い未処理ノーツを優先)" (the nearest unjudged note) and "最下ノーツ最優先" (always the lowest). Describes
  beatoraja's options, not a measurement of LR2.
- **S18** BMS Community, "resources", <https://bms-community.github.io/resources>: Bokutachi "Supports beatoraja (PMS
  only), LR2oraja (including Endless Dream) and LR2."

S4 (re-read 2026-11) describes beatoraja's three note choices: コンボ優先 takes the note a GOOD or better can take,
and when both or neither can, the lower; "LR2に近い処理（らしい）" (reportedly close to LR2). スコア優先 the same with
GREAT; 最下ノーツ優先 always the lower.

Searched without a further find (2026-11): LR2 仕様, ゲージ 減少量, TOTAL 未定義, 地雷 ダメージ, 判定 優先 ノーツ,
#RANK 4 in LR2, Tachi's docs on LR2. A summarising search pointed at bmz-player's `docs/rule.md` for `#RANK 4`;
it was not opened (bmz-player is off limits here).
