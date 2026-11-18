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
A chart without `#RANK` plays NORMAL, as one whose `#RANK` is out of range (below 0, above 4) or not a number;
a bmson `judge_rank` of 0 or less is 100 %. bmz-player's documentation (`docs/rule.md`, "Beatoraja") says
beatoraja judges a chart without `#RANK` at 100 % (EASY) instead. That is the only public description found:
beatoraja's own manual (`manual/extension.txt`) covers `#RANK 4` and `#DEFEXRANK` but not a missing `#RANK`, and
the community write-ups read for this list (S2, S3, S4, S13, the mBMplay patch notes, beatoraja issue 468,
the BMS-making wikis) say nothing about it. One source is not enough to change how every chart without `#RANK`
judges, so vibeBMS keeps NORMAL here until a second, independent description (or a measurement) agrees.

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

### IIDX (the arcade)

The IIDX preset follows the arcade game (beatmania IIDX on the cabinet and INFINITAS) as public write-ups describe
it; Konami publishes no windows. Every key mode but 9 keys, independent of `#RANK`, in ms (120 Hz frames):

| | PGREAT | GREAT | GOOD | BAD | empty POOR |
|---|---|---|---|---|---|
| notes, scratch | ±16.67 (2) | ±33.33 (4) | ±116.67 (14) | ±250 (30) | -150 (18) .. +500 (60) |
| CN / BSS end | ±116.67 (14) | – | – | ±250 (30) | – |

- **PGREAT, GREAT, GOOD**: ±1, ±2 and ±7 frames of the 60 Hz game [S20, S23, S24, S25; S31 for PGREAT]. S23 reads
  each chart's own windows from the game data (Resort Anthem and Lincle): the default is symmetric about the PGREAT's
  two frames, and some old charts have their own (GAMBOL, the "group D" songs one GREAT frame wider each side);
  vibeBMS has the default only. Alternative not taken: windows wider on the FAST side (GREAT +50 / -33.33, GOOD
  +133.3 / -116.67), an unsourced claim relayed by S28; S20 calls asymmetric windows a myth.
- **BAD**: ±250 ms, 15 frames at 60 Hz [S20; S28 relays the same]. Alternatives: S23's default windows put the BAD
  edges 17 frames out (±283 ms; Lincle-era data); DX MODE ±200 [S19].
- **Frames**: the game judges once a frame, 60 Hz until IIDX 26 [S26, S23: "the timing windows are still specified
  in frames" after GOLD's engine change]; the LIGHTNING MODEL cabinet (IIDX 27 HEROIC VERSE on) runs at 120 Hz with
  the same windows in time [S31; S27 expected it before the cabinet came out], as INFINITAS can ("running the game as
  120Hz" [S3]). vibeBMS quantises input timing to the 120 Hz grid in every era.
- **Empty POOR**: its window is not known [S20]. Early, it is a press "slightly before the FAST BAD window" that
  leaves the note to be judged [S20, S29]; late, a second press soon after a note was judged, unless it is within the
  next note's BAD window [S20, S29]. vibeBMS keeps 500 ms early and 150 ms late (beatoraja's 7-key empty POOR, which
  the IIDX preset had before DX MODE): unverified. It keeps the combo [S30 (Konami), S20, S21], in 5 keys too.
  Alternative not taken: an early empty POOR breaks the combo, seen on IIDX 19 Lincle [S29] and repeated as hearsay
  in 2022 [S25]; the current sources say it does not.
- **BAD takes the note** [S20: a press in the BAD window "counts as judgement for that note, and erases the note"];
  BAD and POOR break the combo [S30, S20, S21].
- **CN / BSS**: the head is judged as a note; the end has only PGREAT, BAD and POOR [S21]: a release within the GOOD
  window is a PGREAT ("GREAT and GOODs get automatically upgraded to PGREAT" [S20]; a window "considerably wider than a
  note's PGREAT" [S21]), within the BAD window a BAD, earlier a POOR [S21]. A BSS ends with the turntable turned the
  other way (or stopped [S21]). Not followed, one source each: no end judgement at all after a BAD or POOR head
  [S20]; holding past the end a BAD [S21] (vibeBMS: POOR, as in the other presets).
- **HCN**: the gauge rises while held and falls while not, an empty POOR "basically every 16th note" [S21]; vibeBMS
  keeps its own ticks (every 200 ms, GREAT × 0.5 or BAD × 0.5; "Long notes"): unverified against the arcade.

9 keys (pop'n music's, POP): pop'n's COOL is ±25 ms on every chart [S31, S34]; its GREAT, GOOD and BAD windows vary
by chart [S32, S34] and no source gives them, so vibeBMS keeps DX MODE's POP windows [S19], in ms and not quantised:
PGREAT ±25, GREAT ±50, GOOD ±87.5, BAD ±100, empty POOR -112.5 .. +500; CN / HCN end ±120 / ±150 / ±217 / ±283 with
a 200 ms early-release margin (unverified). A BAD does not take the note, an empty POOR (pop'n's 空打ちBAD) breaks
the combo [S32: "IIDXとは異なり、空打ちBADでもコンボは切れる"] and a note gives one BAD at most [S32] (as PMS under
beatoraja).

DX MODE values differ (S19, the preset's values from replay format 6 to 7): BAD ±200 ms, the empty POOR 200 ms late,
the CN / BSS end's BAD ±200; and its MultiBad and late-BAD rule for LN heads, which vibeBMS never took.

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
| P-ATTACK | 100 | – | 0.1, -1, -100, -100, -100, -100 | ×0.6 below 32 % |
| G-ATTACK | 100 | – | -10, -1, 0.1, -6, -10, -2 | LR2 damage on PG, POOR, empty POOR; ×0.6 below 32 % |

The survival gauges (HARD to EXHARD-CLASS, P-ATTACK, G-ATTACK) empty when they fall below 2 %; GROOVE and EASY
never fall below 2 %. The × 0.6 rule takes only BAD, POOR and empty POOR: G-ATTACK's PGREAT and the attack gauges'
GREAT are never reduced.
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

P-ATTACK and G-ATTACK [S-OLR2 3.2, S-OLR2b 1] are the LR2 set's alone. P-ATTACK dies on a GOOD or worse and on an
empty POOR (−100, or −60 at 30 % shown or less, more than the gauge then holds), so a P-ATTACK clear is always a FULL
COMBO; G-ATTACK heals only on a GOOD (+0.1) and pays −10 × the LR2 damage factor for a PGREAT, as for a POOR. Both
start at 100, empty below 2 %, have no border and clear (HARD CLEAR) at any value they live at; mines take their value.
The gauge option cycles them in LR2's order under the LR2 set: GROOVE, HARD, HAZARD, EASY, P-ATTACK, G-ATTACK, then the
set's ASSIST EASY and EX-HARD (vibeBMS's); the other sets do not offer them, and play one chosen under the LR2 set as
HARD. A play adds the attack gauge it is on to the set's nine (it is simulated with them); every other play has the
nine as before. They are drawn as HARD (no colour of their own, no 80 % border, the "clear" colours on every segment
[S-OLR2b 1.3]): a skin's gauge type for them is HARD's (beatoraja's index 3, `skinvals::skin_gauge`), LR2's skin
options 122 and 123 are P-ATTACK and G-ATTACK, 119 (survival) and 43 hold for both.

HAZARD is LR2's DEATH gauge: no gains, a BAD or POOR ends the play, an empty POOR costs nothing [S8, S-OLR2 3.5],
and it clears as GROOVE does, at 80 %. CLASS is LR2's class (dan) course gauge for GROOVE and EASY, EX-CLASS its
gauge for HARD (with HARD's damage) [S-OLR2 3.6]; a class course switches EASY and G-ATTACK to GROOVE, so the class row is
GROOVE's only [S-OLR2b 5.1]. ASSIST EASY, EX-HARD and EXHARD-CLASS are not LR2's [S8]: the set offers them, but the
vibe-LR2 profile does not take them (gap A10). LR2's course gauges are below ("LR2 courses").

### LR2 courses

LR2's three course kinds [S-OLR2b; S-OLR2 3.6]. vibeBMS's difficulty-table courses are class courses; the player's own
are made in the settings' COURSES page (a chart added with C in the song select), drawn by RANDOM MIX (expert and
NONSTOP, LR2's settings) or imported from an LR2 `song.db` (its `expert`, `nonstop` and `grade` tables, read by
`lr2db`, a small SQLite reader of vibeBMS's own that builds for the web too; LR2's `.lr2folder` course folders only
name those tables, and carry no course). At most 5 stages.

Identity [S-OLR2b 2.3]: LR2's packed course string, a 32-character header (a join digit per join, the kind digit at
11, zeros) and the stages' MD5s. Its characters 29 to 32 are in LR2 the start of the MD5 of the player's password;
vibeBMS has none and writes `0000`, so a course is the same for every player and strings can be exchanged (an LR2
player's string is read whatever those four are; written again it has `0000`). Records are kept under it, as a
song's.

Gauges under the LR2 set (`GrooveGauge::new_in`): every gauge starts at 100, has no 2 % floor for judgements
(GROOVE and EASY empty below 2 % too), takes the × 0.6 rule and clears at any value it lives at (no 80 % in a course
[S-OLR2b, correcting S-OLR2 3.1]); a mine still stops GROOVE and EASY at 2 % [S-OLR2 3.6, inferred]. Expert and
NONSTOP: GROOVE 0.1, 0.1, 0.04, −1.5, −2, −1.5; EASY 0.12, 0.12, 0.048, −1.2, −1.6, −1.2; HARD, HAZARD and the attack
gauges as single play. Class: CLASS (GROOVE's) and EX-CLASS (HARD's) above, HAZARD and P-ATTACK as single play. Under
the other gauge sets every course plays the set's dan gauges, as vibeBMS's table courses always did.

- **Class** [S-OLR2b 5]: under the LR2 gauge set a class course forces LR2's settings for its stages
  (`RuleOptions::for_course`): EASY and G-ATTACK play GROOVE, no lane option, BATTLE, DP FLIP, AUTO SCRATCH, LEGACY
  NOTE, NO MINE or EXPAND JUDGE, no lane cover, SUDDEN or HIDDEN; hi-speed and timing as set. LR2 changes the
  player's own settings and keeps them changed (inferred); vibeBMS changes them for the course only. Under the other
  sets a table course's constraints apply as before (`grade_mirror`, `gauge_lr2`, an LN mode).
- **Expert** [S-OLR2b 4]: a play per stage, its stage result between; the gauges carried over with their exact
  values, the course combo (shown in play) and the course's totals (empty POORs in, mines out), and the seed of the
  first stage; failing a stage (or quitting it) ends the course. A stage's lamp: LR2's rule with the course gauges.
  Each stage goes into its chart's own record partly: best EX and counts (strictly higher), max combo, min BP, the
  lamp only to FULL COMBO, no play count. The course's result follows the last stage (or the failed one): its lamp is
  FAILED below 2 or unfinished, else FULL COMBO when the course's max combo is its notes, else the gauge's lamp when
  it ends strictly above 2 (2.0 fails), no lamp cap; under the other sets the worst stage's. The notes of stages not
  played count as not judged (min BP). No retry. Class courses are expert courses with the settings above.
- **NONSTOP** [S-OLR2b 3]: the stages joined into one chart (`nonstop::join`) and played as one: stage N+1's bar
  line nearest its first note (a half rounding down; gap 1: 0.5 to 1.5 measures before it; gap 2: its start) is put
  on stage N's bar line nearest stage N's last note (a tie: the earlier; gap 2: the first more than 5 s on); stage N's
  bar lines and BPM changes end there; stage N+1's notes at or before stage N's last note + 100 ms are background
  sounds, but one within 100 ms of it on a lane stage N left free in its last 100 ms. Tempo-matched joins play stage
  N+1 at stage N's BPM at the join over its own `#BPM` (its sounds at that speed and pitch, resampled); a stage with no
  `#BPM` line is not matched. Each stage's sound fades in before its join and out after the next (the six join
  types' times; linear, squared: each stage a mixer volume group) and the BGA of two stages cross-fades over 3 s each
  side. `#BPMxx` and `#STOPxx` are shared (a later stage's replaces one of the same number), the `#RANDOM` sequence is
  seeded once (a retry keeps it). The scroll goes by time, as a 150 BPM chart at the hi-speed set (HS-FIX off; a STOP
  still holds it; no lamp cap). Windows: the first stage's `#RANK` as usual; a later stage's `#RANK` switches them
  from that stage's start to LR2's NONSTOP table (0 → 12 / 24 / 60, 1 → 15 / 30 / 80, 3 → 21 / 60 / 120, other → 18 /
  40 / 100, BAD 200 ms); a stage without `#RANK` keeps them. TOTAL: the last `#TOTAL` in stage order (below 1 or
  none: the default of all stages' notes); the HARD factor from it and the notes. One combo, one gauge, a fail ends
  it. One result on the normal result screen, with the normal lamp caps, saved as a song's under the course's
  identity; its replay is kept as LR2 names it, by the MD5 of the identity. Retry plays the course again.

Deliberately not as LR2 [S-OLR2b marks each ambiguous or an artefact]: a NONSTOP stage's mines keep their own value
(LR2 adds 1296 a stage: an instant fail, or 2 % on GROOVE / EASY); every NONSTOP stage is placed by the second stage's
rule (LR2 places the third and later ones a stage's trim earlier); the current stage (skin options 280-283, the
title shown) advances at every join (LR2: at gap joins only); the open `#IF` state is each stage's own (LR2 carries it
on); an expert course's score is recomputed from its totals, its notes the sum of all stages' (LR2 `main`'s rule).
Not followed: LR2's maniac judge options (vibeBMS has none); the `ir` course flag is
kept, but nothing is sent to an internet ranking (a course play is no IR's); a NONSTOP stage whose `#BPMxx` came before
its `#BPM` line is scaled with it; a NONSTOP mine converted by the overlap clean-up is dropped, not played; replays of
an expert stage are kept (with the gauges it started from) but the song select does not list them.

### IIDX set (the arcade)

Values per PG, GR, GD, BAD, POOR, empty POOR. The groove gauges start at 22 % [S20] and never fall below 2 %; the
survival gauges start at 100 % and fail when they reach 0 % [S20, S21, S29]:

| gauge | init | border | values | | sources |
|---|---|---|---|---|---|
| ASSIST EASY | 22 | 60 | a, a, a/2, -1.6, -4.8, -1.6 | | S20, S21 |
| EASY | 22 | 80 | a, a, a/2, -1.6, -4.8, -1.6 | | S20, S21 |
| NORMAL | 22 | 80 | a, a, a/2, -2, -6, -2 | | S20, S21 |
| HARD | 100 | – | 0.16, 0.16, 0, -5, -9, -5 | damage halved at or below 30 % | S20, S21, S22, S29 |
| EX-HARD | 100 | – | 0.16, 0.16, 0, -10, -18, -10 | | S20, S21, S22, S29 |
| HAZARD | 100 | – | 0.16, 0.16, 0, -100, -100, -10 | | (see below) |
| CLASS (段位) | 100 | – | 0.16, 0.16, 0.04, -1.5, -2.5, -1.5 | damage halved at or below 30 % | S20, S21, S29 |
| EX-CLASS (EX段位) | 100 | – | 0.16, 0.16, 0.04, -3, -5, -3 | | S20, S21 |
| EXHARD-CLASS | 100 | – | 0.16, 0.16, 0.04, -6, -10, -6 | | none (vibeBMS's) |

- **a**, the groove gauges' gain, depends on the note count alone in IIDX, which has no TOTAL [S20, S21]. S20's graph
  gives a = 260 / n up to 338 notes and 760.5 / (n + 650) above, rounded to 0.02 (its table: 0.76 for 338 to 364
  notes, … 0.16 for 3824 to 4420, 0 from 75401). That is a × n = max(260, 7.605 n / (0.01 n + 6.5)), the default
  TOTAL vibeBMS already uses for a chart without one. vibeBMS keeps its approach: a = TOTAL / notes, the chart's TOTAL
  where it has one (a BMS's `#TOTAL` takes the place of IIDX's note-count rule), else that default. Not taken: S20's
  rounding of a to 0.02 (S20 alone; it would move every chart's TOTAL a little).
- **The 30 % rule**: at or below 30 % a BAD, POOR or empty POOR takes half [S20: "at or below 30%", with HARD 28 %
  and a POOR losing 4.5]; S21 says 30 % or less, and that it really sets in a little above 30 % (so that a miss in
  the low 30s does not cost much more than one at 28 %), without a value: vibeBMS takes 30 % itself. EX-HARD and
  EX-CLASS have no such rule [S20, S21]. In S21's words the dan GOOD is "25 % of a GREAT's" (0.04).
- **Death at 0 %**: S20 ("get a stage fail if you drop to 0%"), S21 (HARD clears if the gauge never reaches 0 %),
  S35 (the expert course gauge: "When the gauge hits 0% at any point, it is a game over"). vibeBMS's survival gauges fail when they reach 0 % (the gauge cannot go lower).
- **HAZARD**: the arcade's HAZARD mode fails a play on a combo break [S35: "a single combo break will trigger instant
  stage failure"]; an empty POOR breaks no combo [S30], so vibeBMS's HAZARD here is
  EX-HARD with BAD and POOR -100 (unverified beyond that).
- **EX-CLASS** is the arcade's EX段位 gauge, removed in IIDX 32 Pinky Crush [S21]; **EXHARD-CLASS** has no arcade
  counterpart and keeps the values it had (unverified).

The sources agree on every value above. One alternative: S25 gives NORMAL's BAD and empty POOR as -1.6 (EASY's), against
S20 and S21's -2.

9 keys play pop'n's (POP) gauges, unchanged: ASSIST EASY to EX-HARD are groove gauges (2 to 120 %, from 30 %, cleared
at 65 % for ASSIST EASY and 85 % for the others; gains 1.2, 1.2, 0.6 × TOTAL / notes; damage per BAD, POOR, empty POOR
-0.937, -2.929, -2.929 on ASSIST EASY and EASY, -1.992, -5.976, -5.976 on NORMAL, -3.984, -11.952, -11.952 on HARD,
-7.968, -23.906, -23.906 on EX-HARD); HAZARD 0.15, 0.06, 0, -100, -100, -100; CLASS 0.15, 0.15, 0.06, -1.5, -3, -3
(halved below 30 %); EX-CLASS 0.15, 0.15, 0.03, -3, -6, -6; EXHARD-CLASS 0.15, 0.15, 0, -5, -10, -10; no 2 % rule.
These are the arcade pop'n gauge [S32] on a 120 % scale (24 segments of 5 %): pop'n's gauge has 1024 steps, starts
at 256 (6 segments, 30 %), clears from 725 (17 segments, 85 %) and is full at 24 segments [S32; S33 (Konami) names
segment counts]; a COOL or GREAT adds floor(3072 / n) steps (vibeBMS: 1.2 × TOTAL / n %, which is that with TOTAL 300
before the floor), a GOOD half of that; a BAD (ズレBAD) takes 17 steps on NORMAL (1.992 %), a POOR or 空打ちBAD 51
(5.976 %), EASY 8 and 25, HARD 68 and 204 [S32]. pop'n's DANGER (a BAD empties the gauge [S32, S33]) is not EX-HARD's
doubled HARD (DX MODE's [S19], kept); ASSIST EASY, HAZARD and the CLASS rows are not pop'n's (kept from DX MODE, with
its 30 % rule on CLASS; unverified). From 1025 notes the floor makes pop'n's gain visibly smaller than 3072 / n, and
from 1537 notes it is 1 step with GOOD also 1 [S32]; vibeBMS does not floor.

DX MODE values differ (S19, the preset's values from replay format 6 to 7): HARD -4.5, -9, -4.5 halved below 30 %,
EX-HARD -9, -18, -9, HAZARD's empty POOR -9, and every survival gauge failing below 2 %. Records stay under the IIDX
rule mode whatever the version; an IIDX-preset replay of format 6 (DX MODE) or 5 and before (an earlier approximation
of the arcade) may judge otherwise when played again.

## Long notes
* **LN** (`#LNMODE 1`, default): only the start is judged; the final judgement is the worse of the start and
  the release (release windows above). Releasing earlier than the BAD release window gives BAD. The LR2 judge
  has LR2's LN instead (above).
* **CN** (`#LNMODE 2`): start and end are judged separately (counts as 2 notes). Holding more than the BAD
  late window past the end gives POOR.
* **HCN** (`#LNMODE 3`): CN plus a gauge tick every 200 ms of the body: +GREAT×0.5 while held, BAD×0.5 while released.
* Back-spin scratch: on a scratch CN/HCN the end is judged when the turntable is spun the other way. For
  keyboards and touch, releasing also ends it (configurable in the engine).
* LN MODE is AUTO (LN, CN, HCN: the type of the long notes the chart leaves untyped; a type the chart gives
  wins) or FORCE (FORCE LN, CN, HCN: every long note plays as that type, whatever the chart gives; bmz-player's
  `docs/ln.md`). FORCE takes the types off when the chart is arranged (`play::arrange`).
* Scores, replays and IR entries are filed by LN mode: a chart without long notes under LN; under FORCE, the
  forced type; a chart whose long notes all have their type (`#LNMODE`, bmson `t`) under that type (with several
  types, the strongest: HCN > CN > LN), whatever the player's AUTO mode; otherwise under the player's LN mode.
  Records such charts had under the player's mode are merged into the chart's when the library loads. A FORCE
  play is filed with the AUTO plays of its type, and on a chart with both untyped and typed long notes they may
  play differently (bmz-player keeps AUTO and FORCE apart there; vibeBMS's records have the three LN types only).
  G-BATTLE takes a replay only when its long notes play as the player's would (`LnProfile::plays_alike`).
* IR: FORCE LN is LR2's play of a chart that types its long notes (LR2 reads no `#LNMODE`: every long note an
  LN), so the vibe-LR2 profile takes it; vibe-beatoraja takes FORCE only where it changes no type the chart
  gives (beatoraja plays the chart's types). Other FORCE plays are local rules.
* They are filed by rule mode too, the judge preset played with (beatoraja, LR2 or IIDX): each has its own best
  EX score, lamp, min BP, min CB, play counts and replays, and song select shows those of the judge preset chosen
  now. (The gauge set is not part of it: a play with LR2's windows and beatoraja's gauges is an LR2 play.) Course
  records are filed the same way under the course's identity. The IR lamps are the IR's and stay as they are.
  Records from before rule modes are filed under the rule of their best EX play's replay, where it can be read when
  the game starts (desktop and mobile; in the web build the replays are not read yet then), else under beatoraja,
  the default preset; replays from before count for the preset they were played with.

## Scroll
`#SCROLL` (channel SC) changes how fast the lane scrolls from a beat on (scroll positions are the integral of it).
beatoraja's `#SPEED` (channel SP; bmz-player's `docs/hs.md`) multiplies the whole lane's scroll at the current time:
linear in time between its points, from 1 at time 0 to the first, the last after it. Neither changes a note's
time or the judge. Under CONSTANT the notes go by time, so neither has an effect. A NONSTOP course's scroll goes by
time (above) and drops its stages' `#SCROLL` and `#SPEED`.

## Lane options
Each side's lane option is drawn from the play's seed (2P from seed + 1) with Java's `Random`, so a replay draws
the same lanes (`lane_option.rs`).
* F-RANDOM and MF-RANDOM are bmz-player's (its change log calls them HALF RANDOM and MIRROR HALF RANDOM; its
  documentation does not say more, and this is vibeBMS's reading): the keys left of the middle are shuffled among
  themselves, and those right of it; the middle key (5, 7 and 9 keys) and the scratch stay. MF-RANDOM then swaps
  the halves (the left half's notes are shuffled onto the right), so MIRROR of an F-RANDOM is an MF-RANDOM, as
  bmz-player pairs them. Both are kept and sent as usual (bmz-player's rule, `docs/score-persistence.md`); a
  beatoraja skin shows them as RANDOM, Bokutachi gets RANDOM.
* S-RANDOM interval (`SRandomInterval`, bmz-player's `docs/settings.md` "S-RANDOMの補正"), for S-RANDOM and
  S-RANDOM+: OFF (LR2's, the default and vibeBMS's rule before the setting) looks at no interval, only at notes at
  the same time and held long notes; 40 MS (beatoraja's) takes a lane whose last note ended less than 40 ms before
  last; 120 HZ counts the interval in whole 120 Hz frames (8.33 ms) and takes a lane last below 5 frames, one more
  for each note of the side in a row closer than 8 frames to the one before, up to 8 (bmz-player describes it as
  widening to 6–8 frames on runs; the base and the steps are vibeBMS's reading). A lane taken last is still taken
  when no other is free. It does not change how a play is saved or sent; replays keep it.
* TRAINER (bmz-player's Random Trainer, `docs/settings.md`): the side's keys in the player's order
  (`RuleOptions::trainer_order`, the keys from the left as digits), not drawn. bmz-player keeps it for 7 keys 1P
  and calls it a practice tool; vibeBMS plays it on either side of any chart whose key count is the order's, and
  as OFF elsewhere. A RANDOM's lanes picked by the player are not a RANDOM play, so TRAINER is a light assist
  (L-ASSIST, the lamp and counts only, no IR) where it took effect; replays keep the order.

## Chart modifiers
bmz-player lists SCROLL REMOVE / ADD, LONGNOTE REMOVE / ADD LN, CN, HCN and MINE REMOVE / ADD RANDOM, NEAR, BLANK
(`docs/select-detail-options.md`) without saying what the ADD ones add; vibeBMS has LONGNOTE REMOVE (LEGACY NOTE) and
MINE REMOVE (NO MINE), and these, applied after the lane options (`lane_option.rs`):
* SCROLL REMOVE: `#SCROLL` and `#SPEED` off (the notes at their beats). Not in a NONSTOP course (its scroll goes
  by time).
* LONGNOTE ADD LN / CN / HCN: each normal note a long note of that type, ending half way to the next object of its
  lane, at most a beat on (a beat after its lane's last note); none shorter than a 32nd note (it stays a note).
  LN MODE FORCE plays them as the forced type.
* MINE ADD, drawn from the play's seed, at each time a side has notes, on its keys that are free then (no note at
  that time, no long note held; never the scratch): RANDOM one key drawn at random, NEAR the keys beside each
  note, BLANK every free key. Each mine takes 2 % (`ADDED_MINE_DAMAGE`). It comes after LONGNOTE ADD (no mine in an
  added long note's body) and NO MINE (the chart's mines removed, the added ones kept).
Their assist levels are above ("Clear lamps"). LR2's class courses switch them off; G-BATTLE plays neither ADD (each
side's would make two charts). Replays keep them.

## FREQ
FREQ (LR2's audio pitch option, as bmz-player documents it: `docs/settings.md` "FREQ") plays the whole chart
s semitones higher, −12 ≤ s ≤ 12, at 2^(s / 12) times the speed (`play::freq_rate`; shown rounded, `+3 (119%)`).
The chart is retimed once it is read (`practice::retime`: every time ÷ the rate, BPMs × it, stops and sound slices ÷
it) and its sounds are resampled, so the pitch goes with the speed. The judge windows are in real time: the same
milliseconds at every FREQ, so a faster play is harder. The lane options run on the retimed chart (H-RANDOM's and
S-RANDOM's intervals are real time).
* Below 0: the play is not saved at all, as practice is (no score, lamp, play count, replay or IR).
* Above 0: saved as usual and filed with FREQ 0's plays (it is harder, the lamp and the score are earned at
  least as well). Not sent to any IR, vibeBMS's own included (`Refusal::Freq`): no IR files a play by its speed.
* Courses (every kind; LR2 switches it off in class and NONSTOP courses [S-OLR2b 3.8, 5.1]) and practice (it has its
  own speed) play at 0. G-BATTLE plays at the opponent's replay's FREQ.
* Replays keep it (a replay of before has 0) and play at it again; the play record (`RuleFacts::freq`) too.

## Best scores
The record of a chart keeps, apart from each other: the best EX score (with that play's judgements), the best lamp,
the max combo, the min BP (BAD + POOR + empty POOR + notes not reached, skin number 76) and the min CB (combo
breaks, the notes a failed play never reached counted; vibeBMS skin number 9425, as 425 is the play's). An
assisted play keeps its lamp and counts only. Records from before the min CB was kept show 0 until a play sets it.

## Clear lamps
FAILED < ASSIST (assist options) < L-ASSIST (light assist options, ASSIST EASY gauge) < EASY < CLEAR < HARD <
EX-HARD < FULL COMBO (no BAD/POOR; HAZARD survival) < PERFECT (no GOOD) < MAX (all PGREAT). DJ level thresholds
at n/9 of max EX.

Assists (as beatoraja's `BMSPlayer`; an option counts only when it took effect, `AssistFacts` in `score.rs`):
* ASSIST: AUTO SCRATCH on a chart with scratch notes, LEGACY NOTE on a chart with long notes, EXPAND JUDGE over 100 %.
* ASSIST also: LONGNOTE ADD that made long notes (as LEGACY NOTE, the option it undoes: the chart's notes change).
* LIGHT ASSIST: SPIRAL, H-RANDOM, ALL-SCR, RANDOM+, S-RANDOM+, TRAINER on a chart of its key count, BATTLE, NO MINE
  that removed mines, MINE ADD that added mines (as NO MINE), SCROLL REMOVE that took scroll changes off (a reading
  aid, as BPM GUIDE), BPM GUIDE on a
  chart that changes BPM.

An assisted play that is not FAILED gets that lamp whatever the gauge. It keeps only the lamp, the play and clear
counts and the player's totals: no EX score, BP or combo best, no best replay (MY BEST ghost), nothing sent to IR.

A play whose rules are the vibe-LR2 profile's gets LR2's lamp instead, one of its five [S-OLR2 7.2]: FAILED if the
gauge emptied; else FULL COMBO if the max combo equals the notes, whatever the gauge ends at (a GROOVE full combo
at 40 % is one; empty POORs and mines keep the combo); else HARD (HARD, P-ATTACK, G-ATTACK), CLEAR (GROOVE, HAZARD at
80 % or more; in a course at any value they live at), EASY (EASY at 80 % or more), or FAILED. Any assist caps it at EASY, a full combo included. It is the lamp kept
and the one IR is sent. The EX score (2 × PGREAT + GREAT) and the DJ level (k / 9 of the max EX) are LR2's
[S-OLR2 7.1]; LR2's 200 000-point score is not shown.

## LR2 equivalence (the vibe-LR2 profile)

The vibe-LR2 rule profile (`crates/vibebms-ir/src/rule.rs`) is sent to a third-party IR as LR2 rules only for
plays no open gap of its equivalence list reaches. A gap closes when a source establishes LR2's behaviour and the
engine follows it under the LR2 preset (or the profile leaves the case out); a gap is narrowed when its reach is
cut down to the plays it can change, which are then refused with the gap's reason. Nothing here comes from the
source of beatoraja, LR2oraja or bmz-player, or from LR2's binary: S-OLR2 (and S-OLR2b) is a behaviour specification (facts, no
code) that someone else wrote from a reading of OpenLR2, a decompilation-based rewrite of LR2, and vibeBMS was
written from it alone (clean room). `tools/lr2-measure/` holds test charts and steps for a player with LR2 to
check it.

| gap | what | state |
|---|---|---|
| A1 | judge and gauge presets were separate settings | closed: the profile takes only the LR2 judge with the LR2 gauge set |
| A2 | empty POOR window and its combo and note rules | closed: more than 200 and less than 1000 ms early, never late [S1, S2, S3, S4, S-OLR2]; keeps the combo, never takes the note [S4, S-OLR2] |
| A3 | `#RANK 4`, out-of-range and non-numeric `#RANK`, `#DEFEXRANK`, bmson | closed: LR2 reads `#RANK`'s leading digits, 0 when there are none, and plays every value but 0, 1, 3 NORMAL [S-OLR2; S13 and S16 agree for `#RANK 4`]; vibeBMS does the same. LR2 has no `#DEFEXRANK` [S9, S-OLR2] or bmson, and the profile leaves both out |
| A4 | HARD gauge damage factor and the 30 % rule | narrowed to HARD on charts of 21 .. 29 notes where the note-count factor is the larger: S-OLR2 marks that segment (10 − 0.2 a note above 20, then 7 at 30) unchecked; a continuous 10 − 0.3 a note would differ. The kit's `a4-n29` and `a4-n30` settle it (50 vs 56 at 29 notes). Everything else: the base values, BAD = POOR × 0.6, empty POOR = POOR × 0.2, the larger of the two factors, × 0.6 at 30 % shown or less, empty below 2 % [S1, S3, S7, S-OLR2]; the TOTAL factor [S-OLR2; S1's measurements agree, 200 shows the same, above]; the note-count factor from 30 notes up [S-OLR2; S1 from 125 up]. HAZARD is LR2's DEATH gauge [S8, S-OLR2] |
| A5 | LN mode and typed CN / HCN | closed: LR2 has only LNs without a tail judgement [S8, S-OLR2]; the profile takes LN mode and no typed CN / HCN played as typed (LN MODE FORCE LN plays them as LR2 does, and is taken); LR2's LN rules above [S-OLR2; S4, S7 for the release window] |
| A6 | combo, note consumption and one-POOR rules | closed: LR2's in every key mode [S4, S8, S-OLR2] |
| A7 | default TOTAL | closed: `#TOTAL` an integer; missing or below 1, floor(160 + 0.16 (n + clamp(n − 400, 0, 200))) [S-OLR2; the formula as S13 gives it, without the floor] |
| A8 | mines, scratch long note ends | closed: the value as percent on every gauge, held as it passes or pressed up to GREAT early, one POOR [S-OLR2; S14 has the damage and the press, with GOOD for GREAT]. S9 and S15 (value / 2 %, nanasi's) and S10's GROOVE readings (−8, −8, −10, −12, −16 % for 1, 2, 3, 5, 10 %) do not fit; S-OLR2 notes that LR2 may show a mine as 2 POORs on screen while it counts 1, which may be what S10 saw. Scratch LN ends: LN mode only, judged as keys' [S4, S7]. GAUGE AUTO SHIFT: the profile takes none |
| A9 | origin of the LR2 golden vectors | closed: recorded in `judge_golden.rs` and `gauge_golden.rs`; the documented behaviour has its own tests (`lr2_*_as_measured`, `lr2_hard_low_gauge_rule`, `lr2_combo_and_empty_poor_in_every_mode`, `windows_lr2`, `s14_lr2_release_window`, `lr2_note_choice_and_bad_chain`, `lr2_long_notes`, `lr2_mines`) |
| A10 | gauges LR2 does not have | closed: the LR2 set offers ASSIST EASY and EX-HARD (and EXHARD-CLASS in courses), which LR2 has not [S8, S-OLR2]; the set keeps them, but the profile takes only EASY, NORMAL, HARD, HAZARD, P-ATTACK, G-ATTACK, CLASS and EX-CLASS, so a play on the others is local rules |
| A11 | the note a press takes among several in reach | closed: the lane's earliest unresolved object, then the BAD chain [S-OLR2]. The note choice option has no effect under the LR2 judge, and the profile takes any. The earlier sources described beatoraja's "combo" choice as "reportedly" LR2's [S4, S17]; S-OLR2 does not support that. `tools/lr2-measure`'s A11 charts check it (`a11-1`: POOR ≈ 0 with the chain, 16 without) |

What rests on S-OLR2b alone (`docs/LR2_COURSES.md`; no other source describes them): P-ATTACK's and G-ATTACK's
values (S8 and S14 only name them), their option order and look; everything under "LR2 courses" (the course
kinds, the packed string, the expert / NONSTOP gauge values, the joins, fades, tempo match, time-based scroll,
NONSTOP windows and TOTAL, the course lamps and saving, the class settings, RANDOM MIX, `song.db`'s tables). It
corrects S-OLR2: the 80 % clear is outside courses only; the class row is GROOVE's only; "strictly above 2" is for
the expert / class course lamp only (a stage's and a NONSTOP play's lamp need 2); NONSTOP's `#RANK` table changes
only 1P, keeps the windows for a stage without `#RANK`, and switches at the stage's start.

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

Input filters (`SETTINGS › CONTROLS › TURNTABLE AND KEYS`, kept in `[keys.tuning]`):
- **Key chatter filter** (off by default; 1–50 ms): a key or controller button press that comes sooner than this
  after the last press of its lane (and turntable direction) let through is dropped, and so is the release that
  ends it, so presses and releases still pair. It goes by the inputs' own timestamps, not by when a frame sees
  them, so a press is never moved in time: the presses let through are judged at their own times as without it.
  What it changes in judging: a dropped press hits no note and is no empty POOR; two notes of one lane closer than
  the setting cannot both be hit (16th notes at 300 BPM are 50 ms apart; keep it well under the charts' fastest
  jacks); a bounce while a long note is held is a release and a press far from the last press, which it does not
  drop. Touch and analog turntables are not filtered. A replay keeps the presses the judge got, so it is watched
  the same whatever the setting.
- **Analog turntable**: a movement between two reports smaller than the dead band (default 0.004 of the axis's
  −1..1 range; higher is less sensitive) is not a turn, and a turntable that has not moved for the stop time
  (default 100 ms) is let go, its release at the time it last moved plus that time.

Not followed, and why no gap covers it:
- The judging clock. LR2 judges a press on the first game tick that sees it, ticks at most every 2 ms with its
  threaded loop (else once a frame), and has an input debounce; vibeBMS judges each press at its own time, LR2 at
  an unbounded tick rate, and has no debounce by default (the chatter filter below is off). This is deliberate. LR2's tick only delays when a press is seen,
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
- Battle and negative BPM: vibeBMS does not offer them. A negative `#BPMxx` is ignored (the BPM before it goes
  on). LR2 plays it at its absolute value and folds the lane at it: the notes after it are drawn coming back the
  other way, and bmz-player, which follows LR2, folds the judge's time at the same points too (its `docs/hs.md`).
  vibeBMS's timing is one map from beats to time and scroll in which time only goes forward, and the judge reads
  it; a folded lane would need a second, folding time for drawing and judging, which no source describes well
  enough to judge by, so it is left out. LR2's courses are followed (above, "LR2 courses"), but no
  IR takes a course play. Of the IR adapters, Bokutachi cannot file a P-ATTACK or G-ATTACK play (Tachi's BMS gauges
  are EASY, NORMAL, HARD, EX-HARD) and refuses it; BMS-IR reads LR2's attack gauges (its change log: P-ATTACK apart
  from the clear, G-ATTACK clears shown as G-A); rianIR is not known to and refuses them.

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

- **S-OLR2b** `docs/LR2_COURSES.md`: a supplementary clean-room behaviour specification of LR2 beta3 100201 by the
  same method and author as S-OLR2 (P-ATTACK and G-ATTACK, the course kinds and their data, NONSTOP courses, expert
  and class courses), with its corrections to S-OLR2. Not a measurement of LR2.

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
- **S19** bmz-player, `docs/rule.md`, section "DX" (prose; read 2026-11 with its code blocks removed),
  <https://github.com/hyrorre/bmz-player/blob/main/docs/rule.md>: DX MODE as LR2oraja Endless Dream has it. Windows
  for all modes but 9 keys: PGREAT ±16.666, GREAT ±33.333, GOOD ±116.666, BAD ±200 ms, empty POOR 500 ms early and
  200 late (following LR2oraja Endless Dream-rian 1.5.0), LN and long scratch ends ±116.666 to GOOD and ±200 BAD;
  9 keys the POP windows and CN / HCN ends above, a BAD that does not take the note, an empty POOR that breaks the
  combo, a 200 ms release margin. `#RANK` ignored. The IIDX and POP gauge tables above, HARD and CLASS damage halved
  below 30 %, the survival gauges closing below 2 % (not the POP gauges). bmz says its rules were taken from
  LR2oraja Endless Dream's source; it is cited here as DX MODE's documented behaviour.
- **S20** iidx.org, "Life Gauge Calculation and Timing Windows in beatmania IIDX",
  <https://iidx.org/compendium/gauges_and_timing> (read 2026-11, with its two images), and "IIDX LR2 beatoraja
  differences" (S3): PGREAT ±16.67, GREAT ±33.33, GOOD ±116.67, BAD ±250 ms, POOR beyond -250; the empty POOR window
  "?" (a FAST empty POOR "slightly before the FAST BAD window" does not erase the note; a SLOW one is a second press
  soon after a judged note, unless within the next note's BAD window); empty POOR keeps the combo; a press in the BAD
  window judges and erases the note; windows the same before and after ("a myth that the timing windows are
  asymmetric"), the game "based on ticks via quantization"; a CN end has only PGREAT (GREAT and GOOD count as
  PGREAT); after a BAD or POOR head no end judgement. Gauges A-EASY, EASY a, a, a/2, -1.6, -4.8, -1.6, NORMAL -2, -6,
  -2, HARD 0.16, 0.16, 0, -5, -9, -5 halved at or below 30 %, EX-HARD -10, -18, -10, DAN 0.16, 0.16, 0.04, -1.5,
  -2.5, -1.5 halved, EX-DAN -3, -5, -3; groove gauges from 22 % (80 % / 60 % to clear), survival gauges fail at 0 %.
  Its "a-value" graph: a = 260 / x for x ≤ 338 notes, 760.5 / (x + 650) above, with a table in steps of 0.02.
- **S21** beatmania IIDX SP攻略 @ wiki, "ゲーム仕様解説" (as of 32 Pinky Crush; read 2026-11),
  <https://w.atwiki.jp/bemani2sp/pages/4662.html>: NORMAL clears at 80 %, its gain depends on the total notes (GOOD half
  of a GREAT), BAD and empty POOR -2, POOR -6; EASY the same gain, 80 % of NORMAL's damage; ASSISTED EASY as EASY,
  60 %; HARD +0.16 for a GREAT or better, nothing for a GOOD, BAD and empty POOR -5, POOR -9, "30%補正" halving at
  30 % or less ("実際には30％より少し上から発動する"); EX-HARD HARD's gain, twice its damage, no 30 % rule; clears
  if the gauge never reaches 0 %; 段位 GOOD 25 % of a GREAT, -1.5, -2.5, the 30 % rule; EX段位 twice that, no 30 %
  rule, gone in 32 Pinky Crush. CN: head as a note, end only P-GREAT (a window "considerably wider than a note's"),
  BAD (not let go at the end) and POOR (let go before); HCN: the gauge rises while held, falls (basically an empty
  POOR every 16th note) while not; BSS ends turned back or stopped. Empty POOR (空押しPOOR) keeps the combo; BAD and a
  missed POOR break it. Gives no judge windows.
- **S22** "beatmania IIDXでEXHゲージ2%残してFCしたい", <https://note.com/p_300_q/n/n827289c81885>: HARD empty POOR 5,
  BAD 5, POOR 9 %; EX-HARD twice that.
- **S23** Catastrophe, "IIDX Charts With Special Timing Windows", Zenius -I- vanisher.com forum, 2012 (updated 2020),
  <https://zenius-i-vanisher.com/v5.2/viewthread.php?threadid=5233>: read from the game data of Resort Anthem and
  Lincle; each chart may define its GREAT, GOOD and BAD windows in 60 Hz frames, the PGREAT is always "the just frame
  and the one before"; the default definition { -16, -6, -1, 3, 8, 18 } with its figure gives GREAT 4 and GOOD 14
  frames symmetric about the PGREAT's two (±2, ±7 frames) and BAD to 17 frames each side; GOLD's engine "uses
  milliseconds for timing instead of frames. However, the timing windows are still specified in frames"; lists the
  charts with their own windows (GAMBOL, "group D").
- **S24** papk, "beatmaniaIIDXの判定幅は！？ 何フレーム？何ms秒？(2021)", <https://papk.livedoor.blog/archives/4177209.html>:
  PGREAT ±16.67 (±1 frame), GREAT ±33.33 (±2), GOOD ±116.67 (±7) since GOLD; a 20 ms PGREAT is an outdated claim.
- **S25** "【beatmaniaIIDX】初心者の私がつまづいた仕様・システムを全部解説", 2022,
  <https://keionkakimasen.hatenadiary.com/entry/2022/11/12/190000>: PGREAT ±16.67 (±1F), GREAT ±33.33 (±2F), GOOD
  ±116.67 (±7F) at 60 fps; an early empty POOR "reportedly" breaks the combo; NORMAL BAD and empty POOR -1.6, POOR -6.
- **S26** ひつぶ, "【検証】KONAMIの音楽ゲームの判定は正確なのか", 2016, <https://hitsub.blogspot.com/2016/04/konami.html>:
  by video, IIDX's notes are drawn and judged per frame, 60 times a second ("弐寺の判定はフレーム基準").
- **S27** 544332133981, "もしbeatmaniaIIDXが120fpsになったら", 2019-10, <https://544332133981.hatenablog.com/entry/iidx_120fps>:
  before IIDX 27: at 120 fps the windows would keep their time (1 frame at 60 = 2 at 120); reads S23 as PGREAT ±1
  frame and GREAT 5 frames (2 early, 1 late).
- **S28** "【beatoraja】逆エキスパンドジャッジの適切な設定について", 2023,
  <https://my-secret-eau-de-towel.hatenablog.com/entry/20230910/1694332800>: relays, without a source, PGREAT ±16.67,
  GREAT +50 / -33.33, GOOD +133.3 / -116.67, BAD "±0.25ms" (read as ±250), on the 120 Hz frame grid.
- **S29** DBM, "リンクルは空POORでコンボが切れる？", 2011-09, <http://blog.livedoor.jp/dbm_capture/archives/51947661.html>:
  on IIDX 19 Lincle an early empty POOR breaks the combo (the author suspects a bug), a late one does not; an early
  one leaves the note to be judged again; a late one is a press soon after a judged note, the next note first when it
  is within its BAD window. HARD +0.16, +0.16, 0, -5, -5 (early and late empty POOR), -9; EX-HARD -10, -18; 段位 +0.04
  for a GOOD, -1.5, -2.5.
- **S30** Konami, BEMANI PRO LEAGUE 2021, "用語集", <https://p.eagate.573.jp/game/bpl/bpl2021/about/term/index.html>:
  BAD breaks the combo; a missed note's POOR breaks it, a double press's POOR ("空POOR") keeps it; CN and BSS count
  as two notes.
- **S31** "(要検証記事)各踏みゲーの判定幅の比較", <https://w.atwiki.jp/iamkenzen/pages/204.html> (its table from namu.wiki,
  marked unverified there): IIDX PGREAT 16.67, "ライトニング筐体も同じだが120FPSの分だけ綺麗" (the LIGHTNING cabinet
  the same, at 120 fps); pop'n music COOL 25.
- **S32** BEMANIwiki 2nd, "pop'n music Jam＆Fizz/基礎知識",
  <https://bemaniwiki.com/?pop%27n+music+Jam%EF%BC%86Fizz/%E5%9F%BA%E7%A4%8E%E7%9F%A5%E8%AD%98>: COOL the same on every
  chart, GREAT and GOOD per chart; at most one BAD per note; a 空打ちBAD (early press) leaves the note, and unlike IIDX
  breaks the combo; the groove gauge 24 segments, 1024 steps inside, 256 at the start, cleared from 725 (17
  segments); COOL / GREAT floor(3072 / notes), GOOD half (at least 1); NORMAL ズレBAD -17, 空打ち / missed BAD -51,
  EASY -8 / -25, HARD -68 / -204, DANGER all; long pop-kun ends take the head's judgement when held.
- **S33** Konami, pop'n music 29 High☆Cheers!! "遊び方",
  <https://p.eagate.573.jp/game/popn/popn29/howto/most.html>: gauge types NORMAL, EASY (less loss on a BAD), HARD
  (more), DANGER (a BAD empties the gauge); medals for 15-16 and 12-14 segments at the end.
- **S34** "さまざまな音ゲーの判定、スコア計算方法、ランク基準を一挙に比較！", <https://note.com/happy_azalea548/n/n47f151a7b063>:
  IIDX PGREAT 17.00, GREAT 33.33, GOOD 116.66 (citing S21's wiki); pop'n music COOL 25 ms, the other windows per chart.
- **S35** RemyWiki, "beatmania IIDX Arcade General Information", <https://remywiki.com/AC_General_Info>: HAZARD mode
  (EMPRESS on), "a single combo break will trigger instant stage failure"; EXPERT mode's gauge starts at 100 % and
  "when the gauge hits 0% at any point, it is a game over".

S4 (re-read 2026-11) describes beatoraja's three note choices: コンボ優先 takes the note a GOOD or better can take,
and when both or neither can, the lower; "LR2に近い処理（らしい）" (reportedly close to LR2). スコア優先 the same with
GREAT; 最下ノーツ優先 always the lower.

Searched without a further find for the IIDX preset (2026-11): a second source for S20's a-value formula and its
rounding; the arcade empty POOR window; the arcade HCN's damage step; pop'n music's GREAT, GOOD and BAD windows
(BEMANIwiki, RemyWiki, Japanese blogs). Reddit's "Hit windows/timings of Bemani games" could not be read.

Searched without a further find (2026-11): LR2 仕様, ゲージ 減少量, TOTAL 未定義, 地雷 ダメージ, 判定 優先 ノーツ,
#RANK 4 in LR2, Tachi's docs on LR2. A summarising search pointed at bmz-player's `docs/rule.md` for `#RANK 4`;
it was not opened (bmz-player is off limits here).
