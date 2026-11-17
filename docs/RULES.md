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

LR2's windows and judging rules, from LR2's measured windows and S-OLR2, a behaviour specification of LR2
(`docs/LR2_BEHAVIOUR.md`; sources below).

| #RANK | PGREAT | GREAT | GOOD | BAD |
|---|---|---|---|---|
| VERY HARD | ±8 | ±24 | ±40 | ±200 |
| HARD | ±15 | ±30 | ±60 | ±200 |
| NORMAL | ±18 | ±40 | ±100 | ±200 |
| EASY | ±21 | ±60 | ±120 | ±200 |

The rows are LR2's measured windows [S5, S2, S3, S4; S-OLR2 2.1]. A chart without `#RANK` is NORMAL [S8, S9]. LR2
reads `#RANK` as a C integer, its leading digits: `2.5` is 2, `-1` is -1, and a value that is not a number is 0,
VERY HARD; every value but 0, 1 and 3 is NORMAL, `#RANK 4` included [S-OLR2 2.2; S13 for `#RANK 4`]. (vibeBMS
reads the number as it reads every header, so an exponent form such as `1e3` is 1000 here, 1 in LR2; no such
chart is known.) LR2 has no `#DEFEXRANK`: vibeBMS takes it as a percentage of NORMAL, the judge rank then lies
between the rows above (NORMAL = 75 %, EASY = 100 %) and the windows are interpolated between them, in whole ms;
above EASY they grow in proportion (bmson `judge_rank` is taken as that percentage). This is vibeBMS's own rule.
Windows stay inside BAD.

Which note a press judges [S-OLR2 1]: always its lane's earliest object not yet resolved, mines included; never a
nearer or better one (the note choice option does not apply to this judge). A note more than 200 ms late is POOR
before a press is judged, so a late press beyond 200 ms judges the next note. After a BAD the same press also
judges the lane's next object if it is within ±200 ms, and so on while each is a BAD (the BAD chain): one press can
take two notes; an LN so taken is judged and finished at once, a mine within GREAT goes off. A press more than
200 and less than 1000 ms before the note is an empty POOR [S1, S2, S3, S4; S-OLR2 1.4]; one 1000 ms or more
before it judges nothing and plays a keysound. An empty POOR keeps the combo and never takes the note, so one note
can give any number of them; BAD and POOR break the combo [S4, S8]. All this in every key mode [S4, S8, S-OLR2 8].

Long notes [S-OLR2 5; S4, S7 for the release window]: an LN's head is POOR once it is more than GOOD late (not
200 ms). The head's judgement waits: combo, gauge and counts change when the LN ends, held past its end or let go
within the GOOD window before it; let go earlier, the LN is a BAD whatever the head was. There is no late release
window and no tail judgement of its own. Holding the key through the head does not start an LN. A new press while
an LN is held (the turntable turned back, the other scratch key) judges its head again: within a window it
replaces the waiting judgement, else it is an empty POOR. LR2 itself has only such LNs: no CN, no HCN [S8]; under
this judge a CN or HCN (local rules) plays as in the beatoraja preset, its end judged with the GOOD window for
PG..GD.

Mines [S-OLR2 4; S14]: a mine goes off when its key is down as it passes, or is pressed up to the GREAT window
before it; a press while a mine more than GREAT ahead is the lane's earliest object does nothing. It takes its
value (base 36) as percent from every gauge, unscaled; GROOVE and EASY stop at 2 %, every other gauge empties
below 2 %. It counts one POOR (vibeBMS keeps it with the empty POORs: it takes no note) and keeps the combo.

Timing is compared in whole milliseconds, both fractions dropped [S-OLR2 8]. LR2 judges a press on the game tick
that first sees it, polling every 2 ms or more with its threaded loop, else once a frame [S-OLR2 0]; vibeBMS judges
each press at its own time, which is LR2 at an unbounded tick rate (see "LR2 equivalence").

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
with the LR2 gauges LR2's (below). A bmson `total` is a percentage of that default.

**5-key EX-HARD damage modifier** (beatoraja's `MODIFY_DAMAGE`): damage × max(TOTAL factor, note-count factor).
beatoraja's note-count loop also adds negative terms for charts with more notes than the current step, which
effectively disables it; vibeBMS keeps only the positive part.

### LR2 set

| gauge | init | border | values | modifier |
|---|---|---|---|---|
| ASSIST EASY | 20 | 60 | 1.2, 1.2, 0.6, -3.2, -4.8, -1.6 | × TOTAL / notes on gains |
| EASY | 20 | 80 | 1.2, 1.2, 0.6, -3.2, -4.8, -1.6 | × TOTAL / notes |
| NORMAL | 20 | 80 | 1, 1, 0.5, -4, -6, -2 | × TOTAL / notes |
| HARD | 100 | – | 0.1, 0.1, 0.05, -6, -10, -2 | LR2 damage; ×0.6 below 32 % |
| EX-HARD | 100 | – | 0.1, 0.1, 0.05, -12, -20, -2 | LR2 damage |
| HAZARD | 100 | 80 | 0, 0, 0, -100, -100, 0 | ×0.6 below 32 % |
| CLASS | 100 | – | 0.1, 0.1, 0.04, -2, -3, -2 | ×0.6 below 32 % |
| EX-CLASS | 100 | – | 0.1, 0.1, 0.04, -6, -10, -2 | LR2 damage; ×0.6 below 32 % |
| EXHARD-CLASS | 100 | – | 0.1, 0.1, 0.05, -12, -20, -2 | |

The survival gauges (HARD to EXHARD-CLASS) empty when they fall below 2 %; GROOVE and EASY never fall below 2 %.
LR2 damage [S-OLR2 3.4]: damage × the larger of a TOTAL factor, 10 / floor((TOTAL − 80) / 16) with the step at
least 1 (10 below 112, 5 from 112, 3.33 from 128, 2.5 from 144, 2 from 160, 1.67 from 176, 1.43 from 192, 1.25 from
208, 1.11 from 224, 1 from 240), and a note-count factor g(n): 10 below 20 notes, 10 − 0.2 (n − 20) to 29, then
7 − (n − 30) / 15 to 44, 6 − (n − 45) / 15 to 59, 5 − (n − 60) / 65 to 124, 4 − (n − 125) / 125 to 249,
3 − (n − 250) / 250 to 499, 2 − (n − 500) / 500 to 999, 1 from 1000.

Why the TOTAL factor steps every 16 and not at S1's multiples of 10: S1 measured LR2's HARD POOR damage at every
multiple of 10 from 100 to 300 and wrote that LR2 seems to drop the ones digit ("10刻みで設定されている？"). The step-16
rule gives S1's value at every one of them but 200, where S1 has 15 and the rule 14.29. That one cannot be told
apart on LR2's screen, which shows the gauge as 2 × floor(floor(HP) / 2): one POOR from 100 shows 84 either way. S1's
TOTAL 159 for 150's damage fits both. `tools/lr2-measure`'s `a4-t200` (TOTAL 200, 1000 notes, HARD, four POORs)
reads 84 70 56 42 under the rule and 84 70 54 40 under 15.

TOTAL [S-OLR2 3.3]: `#TOTAL` is read as an integer, its fraction dropped (`250.9` is 250); missing or below 1, it is
floor(160 + 0.16 × (n + clamp(n − 400, 0, 200))), n the notes (mines not counted, an LN once): 192 at 200 notes,
256 at 500, 352 at 1000.

HAZARD is LR2's DEATH gauge: no gains, a BAD or POOR ends the play, an empty POOR costs nothing [S8, S-OLR2 3.5],
and it clears as GROOVE does, at 80 %. CLASS is LR2's class (dan) course gauge for GROOVE and EASY, EX-CLASS its
gauge for HARD (with HARD's damage) [S-OLR2 3.6]; in a course every gauge starts at 100, has no 2 % floor and the
×0.6 rule; LR2's expert and NONSTOP course gauges have no counterpart, vibeBMS's courses being dan courses. ASSIST
EASY, EX-HARD and EXHARD-CLASS are not LR2's [S8]: the set offers them, but the vibe-LR2 profile does not take them
(gap A10).

### IIDX set (approximation)
Groove gauges start at 22 %, HARD ±0.16 gain / -5, -9, -5 damage halving below 30 %, EX-HARD -10, -18, -10.
**Approximate values**, adjust in `gauge.rs`.

## Long notes
* **LN** (`#LNMODE 1`, default): only the start is judged; the final judgement is the worse of the start and
  the release (release windows above). Releasing earlier than the BAD release window gives BAD. The LR2 judge
  has LR2's LN instead (above).
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

A play whose rules are the vibe-LR2 profile's gets LR2's lamp instead, one of its five [S-OLR2 7.2]: FAILED if the
gauge emptied; else FULL COMBO if the max combo equals the notes, whatever the gauge ends at (a GROOVE full combo
at 40 % is one; empty POORs and mines keep the combo); else HARD (HARD), CLEAR (GROOVE, HAZARD at 80 % or more),
EASY (EASY at 80 % or more), or FAILED. Any assist caps it at EASY, a full combo included. It is the lamp kept
and the one IR is sent. The EX score (2 × PGREAT + GREAT) and the DJ level (k / 9 of the max EX) are LR2's
[S-OLR2 7.1]; LR2's 200 000-point score is not shown.

## LR2 equivalence (the vibe-LR2 profile)

The vibe-LR2 rule profile (`crates/vibebms-ir/src/rule.rs`) is sent to a third-party IR as LR2 rules only for
plays no open gap of its equivalence list reaches. A gap closes when a source establishes LR2's behaviour and the
engine follows it under the LR2 preset (or the profile leaves the case out); a gap is narrowed when its reach is
cut down to the plays it can change, which are then refused with the gap's reason. Nothing here comes from the
source of beatoraja, LR2oraja or bmz-player, or from LR2's binary: S-OLR2 is a behaviour specification (facts, no
code) that someone else wrote from a reading of OpenLR2, a decompilation-based rewrite of LR2, and vibeBMS was
written from it alone (clean room). `tools/lr2-measure/` holds test charts and steps for a player with LR2 to
check it.

| gap | what | state |
|---|---|---|
| A1 | judge and gauge presets were separate settings | closed: the profile takes only the LR2 judge with the LR2 gauge set |
| A2 | empty POOR window and its combo and note rules | closed: more than 200 and less than 1000 ms early, never late [S1, S2, S3, S4, S-OLR2]; keeps the combo, never takes the note [S4, S-OLR2] |
| A3 | `#RANK 4`, out-of-range and non-numeric `#RANK`, `#DEFEXRANK`, bmson | closed: LR2 reads `#RANK`'s leading digits, 0 when there are none, and plays every value but 0, 1, 3 NORMAL [S-OLR2; S13 and S16 agree for `#RANK 4`]; vibeBMS does the same. LR2 has no `#DEFEXRANK` [S9, S-OLR2] or bmson, and the profile leaves both out |
| A4 | HARD gauge damage factor and the 30 % rule | narrowed to HARD on charts of 21 .. 29 notes where the note-count factor is the larger: S-OLR2 marks that segment (10 − 0.2 a note above 20, then 7 at 30) unchecked; a continuous 10 − 0.3 a note would differ. The kit's `a4-n29` and `a4-n30` settle it (50 vs 56 at 29 notes). Everything else: the base values, BAD = POOR × 0.6, empty POOR = POOR × 0.2, the larger of the two factors, × 0.6 at 30 % shown or less, empty below 2 % [S1, S3, S7, S-OLR2]; the TOTAL factor [S-OLR2; S1's measurements agree, 200 shows the same, above]; the note-count factor from 30 notes up [S-OLR2; S1 from 125 up]. HAZARD is LR2's DEATH gauge [S8, S-OLR2] |
| A5 | LN mode and typed CN / HCN | closed: LR2 has only LNs without a tail judgement [S8, S-OLR2]; the profile takes LN mode and no typed CN / HCN; LR2's LN rules above [S-OLR2; S4, S7 for the release window] |
| A6 | combo, note consumption and one-POOR rules | closed: LR2's in every key mode [S4, S8, S-OLR2] |
| A7 | default TOTAL | closed: `#TOTAL` an integer; missing or below 1, floor(160 + 0.16 (n + clamp(n − 400, 0, 200))) [S-OLR2; the formula as S13 gives it, without the floor] |
| A8 | mines, scratch long note ends | closed: the value as percent on every gauge, held as it passes or pressed up to GREAT early, one POOR [S-OLR2; S14 has the damage and the press, with GOOD for GREAT]. S9 and S15 (value / 2 %, nanasi's) and S10's GROOVE readings (−8, −8, −10, −12, −16 % for 1, 2, 3, 5, 10 %) do not fit; S-OLR2 notes that LR2 may show a mine as 2 POORs on screen while it counts 1, which may be what S10 saw. Scratch LN ends: LN mode only, judged as keys' [S4, S7]. GAUGE AUTO SHIFT: the profile takes none |
| A9 | origin of the LR2 golden vectors | closed: recorded in `judge_golden.rs` and `gauge_golden.rs`; the documented behaviour has its own tests (`lr2_*_as_measured`, `lr2_hard_low_gauge_rule`, `lr2_combo_and_empty_poor_in_every_mode`, `windows_lr2`, `s14_lr2_release_window`, `lr2_note_choice_and_bad_chain`, `lr2_long_notes`, `lr2_mines`) |
| A10 | gauges LR2 does not have | closed: the LR2 set offers ASSIST EASY and EX-HARD (and EXHARD-CLASS in courses), which LR2 has not [S8, S-OLR2]; the set keeps them, but the profile takes only EASY, NORMAL, HARD, HAZARD and CLASS, so a play on the others is local rules |
| A11 | the note a press takes among several in reach | closed: the lane's earliest unresolved object, then the BAD chain [S-OLR2]. The note choice option has no effect under the LR2 judge, and the profile takes any. The earlier sources described beatoraja's "combo" choice as "reportedly" LR2's [S4, S17]; S-OLR2 does not support that. `tools/lr2-measure`'s A11 charts check it (`a11-1`: POOR ≈ 0 with the chain, 16 without) |

What rests on S-OLR2 alone, with no measurement of LR2 behind it: the note choice and the BAD chain (A11); the
empty POOR's 200 ms lower edge (A2); non-numeric and out-of-range `#RANK` (A3; S13 and S16 only hedge `#RANK 4`);
the integer `#TOTAL` and the floor of the default (A7); the note-count factor below 125 notes and the TOTAL factor
at 192 .. 207 (A4); a mine's press window and POOR (A8); LR2's LN head miss, the head judgement recorded at the
end, the re-press during a hold (A5); HAZARD's lack of gains and its 80 % clear; the dan GOOD of 0.04 (S1 writes
"0.05?"); LR2's lamps; whole-millisecond timing. What measurements support as well: the window table [S5, S2, S3,
S4], the empty POOR's 1000 ms edge [S1 to S4], the gauge values and the × 0.6 rule [S1, S2, S3, S6], the TOTAL
factor at the multiples of 10 [S1], the note-count factor from 125 notes up [S1], the LN release window [S4],
the combo rules [S4]. Mine damage has S14 (not said to be measured) and contradicting readings (S10). The kit's
tables give S-OLR2's predictions, which are vibeBMS's rows.

Not followed, and why no gap covers it:
- The judging clock. LR2 judges a press on the first game tick that sees it, ticks at most every 2 ms with its
  threaded loop (else once a frame), and has an input debounce; vibeBMS judges each press at its own time, LR2 at
  an unbounded tick rate, and has no debounce. This is deliberate. LR2's tick only delays when a press is seen,
  by less than one tick (2 ms or more, a frame without the threaded loop), as a device's latency does: it changes
  a judgement only at a window's edge, by the player's setup, not by a scoring rule, and the measured windows
  (S5, taken with vertical sync off) already carry it. Windows are still compared in whole milliseconds as LR2
  does. One consequence of LR2's polling, that a press on the very tick a note is missed is dropped, cannot
  happen at an unbounded tick rate (the note is missed an instant before the press is seen), so it has no
  counterpart.
- Lane inputs: LR2 merges two keys bound to one key lane (a second key pressed while the first is held is no new
  press; one let go while the other is held reads as a release); vibeBMS judges each key. It changes nothing for
  a player with one key per lane; the turntable's two directions are new presses in both.
- The order a lane's objects are resolved in: LR2 checks only its earliest object for a miss, so an LN head or a
  mine just behind a note not yet judged waits for that note; vibeBMS resolves each at its own time. It differs
  only when an LN head is within 200 ms minus GOOD, or a mine within 200 ms, after an unjudged note of its lane.
- Objects that start inside an LN of their lane are dropped by LR2; vibeBMS drops notes there, not mines.
- P-ATTACK and G-ATTACK, battle, LR2's expert and NONSTOP courses (with their own gauge values, `#RANK` table and
  mine offsets) and negative BPM: vibeBMS does not offer them, and the profile takes no course play.

Bokutachi takes LR2 and LR2oraja plays into its BMS 7K and 14K rankings [S18]. With no open gap reaching every
vibe-LR2 play, vibeBMS's Bokutachi BMS adapter is on (`BOKUTACHI_BMS`); nothing is sent until Tachi lists
vibeBMS as a client (`TACHI_ACCEPTS_VIBE`), and A4 refuses the plays it reaches. BMS-IR takes vibe-LR2 plays in
5/7/10/14 keys once its operator allows vibeBMS (not yet).

The beatoraja profile's gaps stay open: no public description was found of beatoraja's 5-key EX-HARD note-count
term (B1) or of how beatoraja ends a back-spin scratch CN / HCN played on a keyboard (B3), and beatoraja's source
is not read. (S4's description of beatoraja's combo choice differs from vibeBMS's Combo when both notes are late
beyond GOOD; it is not on the list, being one reading of one source.)

### Sources

Read for this list (2026-11). "Measured" means the author timed LR2 itself.

- **S-OLR2** `docs/LR2_BEHAVIOUR.md`: a clean-room behaviour specification of LR2 beta3 100201 (its judging,
  gauges, scores and lamps), written by someone else from a reading of OpenLR2 branch `LR2Beta3-v100201`, a
  decompilation-based rewrite of LR2: facts only, no code, each rule labelled clear, inferred or ambiguous, with
  predictions for `tools/lr2-measure`. Not a measurement of LR2.

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
