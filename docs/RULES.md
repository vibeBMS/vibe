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

| #RANK | PGREAT | GREAT | GOOD | BAD |
|---|---|---|---|---|
| VERY HARD | ±8 | ±24 | ±40 | ±200 |
| HARD | ±15 | ±30 | ±60 | ±200 |
| NORMAL | ±18 | ±40 | ±100 | ±200 |
| EASY | ±21 | ±60 | ±120 | ±200 |

LR2 has no `#DEFEXRANK`; it is treated as a percentage of NORMAL. Empty POOR window uses the beatoraja value.

### IIDX (approximation)

Frame based at 60 fps: PG ±16.67, GR ±33.33, GD ±116.67, BAD ±250 ms, independent of #RANK.
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

The 5-key, PMS and LR2 sets are in `gauge.rs` (also from beatoraja's `GaugeProperty`).
Default TOTAL when missing: `max(260, 7.605·n / (0.01·n + 6.5))`.

**LR2 damage modifier** (HARD/EX-HARD of the LR2 set): damage × max(TOTAL factor, note-count factor).
beatoraja's implementation of the note-count factor also adds negative terms for charts with more notes
than the current step (its loop runs `while (note > notes || note > 1)`), which effectively disables it.
vibeBMS keeps only the positive part so that charts with < 1000 notes get the increased damage that LR2 is
known for. Change `ModifyDamage` in `gauge.rs` if you want the exact beatoraja behaviour.

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

## Clear lamps
FAILED < ASSIST (assist options) < L-ASSIST (ASSIST EASY gauge) < EASY < CLEAR < HARD < EX-HARD < FULL COMBO
(no BAD/POOR; HAZARD survival) < PERFECT (no GOOD) < MAX (all PGREAT). DJ level thresholds at n/9 of max EX.
