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

The survival gauges (HARD to EXHARD-CLASS) empty when they fall below 2 %. LR2 damage: damage × max(10 /
clamp(⌊TOTAL / 16⌋ − 5, 1, 10), note-count factor), the note-count factor rising from 1 at 1000 notes to 10 at 20
(1 + 0.002·(1000 − n) below 1000, 2 + 0.004·(500 − n) below 500, 3 + 0.008·(250 − n) below 250, …).

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

The vibe-LR2 rule profile (`crates/vibebms-ir/src/rule.rs`) is sent to an IR as LR2 rules only when every gap of
its equivalence list is closed. A gap closes when public documentation of LR2's behaviour establishes the rule and
the engine follows it under the LR2 preset (or the profile leaves the case out). Nothing here comes from the source
of beatoraja, LR2oraja or bmz-player, or from LR2's binary.

| gap | what | state |
|---|---|---|
| A1 | judge and gauge presets were separate settings | open |
| A2 | empty POOR window and its combo and note rules | closed: 0 .. +1000 ms, early only [S1, S2, S3, S4]; keeps the combo, never takes the note [S4] |
| A3 | `#RANK 4`, out-of-range `#RANK`, `#DEFEXRANK`, bmson | open |
| A4 | HARD gauge damage factor and the 30 % rule | open |
| A5 | LN mode and typed CN / HCN | open |
| A6 | combo, note consumption and one-POOR rules | closed: LR2's in every key mode [S4, S8] (5/10 keys and PMS had beatoraja's) |
| A7 | default TOTAL | open |
| A8 | mines, scratch long note ends | open |
| A9 | origin of the LR2 golden vectors | open |

### Sources

Read for this list (2026-11). "Measured" means the author timed LR2 itself.

- **S1** yoshi_65c816, "LR2の仕様まとめ" (LR2 rules summary), archived
  <https://web.archive.org/web/20190210133959/http://2nd.geocities.jp/yoshi_65c816/bms/LR2.html>. Measured. Empty
  POOR only on early presses; no POOR from presses after a note is hit. Gauge per PG, GR, GD, POOR, BAD, empty POOR
  (t = TOTAL, n = notes): GROOVE t/n, t/n, t/2n, −6, −4, −2; EASY ×1.2 gains, −4.8, −3.2, −1.6; HARD 0.1, 0.1, 0.05,
  −10, −6, −2; dan 0.1, 0.1, 0.05?, −3, −2, −2. The gauge is kept with decimals and shown in 2 % steps. HARD damage
  grows for low TOTAL or few notes: POOR damage measured at TOTAL ≥ 240: 10, 230: 11.1, 220 and 210: 12.5, 200: 15,
  190 and 180: 16.66, 170 and 160: 20, 150: 25, 140 and 130: 33.33, 120: 50, ≤ 110: 100 (the author guesses steps of
  10). Below 1000 notes POOR damage grows 0.02 % a note down to 500 and 0.04 % a note down to 250, then as a graph
  (`file/tn_graph.png`; its marks: 125 notes 40, 0 notes 100). The larger of the two is taken; BAD is POOR × 0.6,
  empty POOR POOR × 0.2. At 30 % (as shown) or less, HARD damage × 0.6.
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
- **S9** hitkey, "BMS command memo", <https://hitkey.nekokan.dyndns.info/cmds.htm> (archived 2022-10-06; read
  through a summarising fetch, so these points want a second look): LR2 applies `#RANK 2` when `#RANK` is missing;
  LR2 is not among the players listed for `#DEFEXRANK`; LR2's default `#TOTAL` is given as "160?"; mines take
  value / 2 % of the gauge and `ZZ` fails the stage.
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
