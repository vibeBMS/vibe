> **Origin:** clean-room behaviour specification written from a reading of OpenLR2 branch LR2Beta3-v100201, a
> decompilation-based rewrite of LR2; facts only, no code. Kept as received below this header, except: the notes
> marked *Corrected by LR2_COURSES.md* (its table "Changes to LR2_BEHAVIOUR.md" says what changes); its comparison
> with RULES.md as that then was, removed (RULES.md "LR2 equivalence" is the live statement); and a note on how it
> was written. `docs/RULES.md` cites it as source S-OLR2.

# LR2 (beta3 100201) judging, gauge and score rules — behaviour specification

Clean-room behaviour specification written from a reading of OpenLR2, branch `LR2Beta3-v100201` (a rewrite of
Lunatic Rave 2 beta3 100201 from a decompilation). It states player-observable rules only. Where the `main` branch
was compared, it is said so; the parts compared (note judging, mines, low-gauge rule, gauge tables, default TOTAL,
HARD damage factor) are the same on `main`, which only adds an optional gauge auto-shift that runs all gauges in
parallel.

Confidence labels:

- **clear**: the rule is stated plainly and consistently; several independent places agree.
- **inferred**: the rule follows from the reading but depends on how two parts interact, or on the meaning of an
  option index.
- **ambiguous**: the rewrite itself flags the spot as unchecked, or it looks like a decompilation artefact, or it
  disagrees with a published measurement.

Conventions used below: "early" means the press comes before the note time, "late" after. All judge windows are in
milliseconds of song time. "Shown gauge" means the gauge value as LR2 displays it (see 3.1).

---

## 0. The judging clock (applies to everything below)

| rule | confidence |
|---|---|
| Judging is done by polling, not by input-event timestamps. On each game tick LR2 reads the current song time (whole milliseconds, fractions dropped) and the current state of every lane's input, then judges each lane against that time. A press is therefore judged at the time of the tick that first sees it, not at the moment the switch closed. | clear |
| With the threaded game loop the tick runs at most once every 2 ms; otherwise once per rendered frame. This is why LR2's effective windows change with frame rate / vertical sync. No window is scaled by BPM or frame rate; the error comes only from when the press is noticed. | clear |
| Each lane has four input states per tick: up, newly pressed, held, newly released. | clear |
| Input debounce ("input interval", default 16 ms): if a switch is let go within this many ms of being pressed, the lane keeps reporting "held" until the interval has passed, then reports "released". A switch that goes down again on the very next poll after a "released" state is reported as "held", not as a new press. | clear |
| When several physical inputs map to one lane, the lane state is the "strongest" of them in the order released > held > newly pressed > up. So pressing a second key on a lane whose other key is held does not create a new press. | clear |
| Turntable: the lane combines the two spin directions with priority newly pressed > held > released. Spinning the other way (or hitting the other scratch key) while one direction is still held therefore counts as a new press. | clear |
| The player's "judge timing" offset moves only where notes are drawn; the judging clock is not shifted by it. The optional auto-adjust nudges that offset by 1 ms after every 10 hits judged GOOD or better (towards the average error). | inferred |

---

## 1. Which note a press is judged against (A11)

### 1.1 The lane cursor

Every lane keeps a cursor at its **earliest object that has not been resolved yet** (notes, long notes and mines all
sit in the same per-lane queue in time order). A press is **always** judged against the object under the cursor.
LR2 never looks past it to pick a nearer or "better" note, with one exception, the BAD chain (1.3). Already-judged
objects are behind the cursor and can never be judged again. (**clear**)

### 1.2 Order of checks for a normal note under the cursor, on every tick

1. **Miss check first.** If the song time is more than the BAD window (200 ms) past the note, the note is judged
   POOR and the cursor moves on. This happens whether or not the key was pressed on this tick, and if a press arrived
   on this same tick it is **discarded**: it is not applied to the next note and gives no empty POOR. (**clear**;
   the lost press is a consequence of the order, **inferred** as player-visible but rare.)
2. If the lane was not newly pressed on this tick, nothing more happens.
3. Let d = |press time − note time| (whole ms, the note time with fractions dropped). The first matching row wins:

| condition | result | note consumed? |
|---|---|---|
| d ≤ PGREAT window | PGREAT | yes |
| d ≤ GREAT window | GREAT | yes |
| d ≤ GOOD window | GOOD | yes |
| d ≤ BAD window (200) | BAD, then the BAD chain (1.3) | yes |
| note is more than 200 ms ahead and **less than 1000 ms** ahead | empty POOR (空POOR) | **no** |
| note is 1000 ms or more ahead | nothing is judged; the lane's last keysound is replayed | no |

All comparisons are inclusive (≤) at whole milliseconds, and the windows are **symmetric** early/late. A late press
beyond 200 ms cannot happen, because the miss check has already moved the cursor; such a press is measured against
the next note instead. (**clear**)

### 1.3 The BAD chain

After a press is judged BAD, LR2 immediately looks at the **next** object in the same lane. If that object is within
±200 ms of the same press time, the same press is judged against it too, by the same rules (so it can be PGREAT,
GREAT, GOOD, or another BAD, which chains again). One press can therefore resolve two or more notes. (**clear** in the
reading; the rewrite marks nothing here.)

Side effects of the chain, **inferred**: if the next object is a mine less than a GREAT window ahead (or already
passed), the chained press sets it off; if the next object is a long-note head, it is judged and finished at once as if
it were a normal note (no hold needed).

### 1.4 Empty POOR (空POOR)

- Window: the note under the cursor is between 200 ms and 1000 ms in the future (more than 200, less than 1000).
  Never late. (**clear**)
- It does not consume the note; any number of empty POORs can be had against the same note. (**clear**)
- It is counted in the POOR count (and therefore in BP), applies the gauge's empty-POOR change, does **not** touch the
  combo, the EX score, the 200 000-point score, or the judged-note count. (**clear**)
- Plays the upcoming note's keysound. (**clear**)
- If the object under the cursor is a mine, a press gives no empty POOR (see 4). (**clear**)

### 1.5 Long-note heads, scratch

- Scratch lanes use exactly the same windows and rules as keys. (**clear**)
- Long-note heads differ (see 5): a head becomes POOR once it is more than the **GOOD** window late (not 200 ms), so a
  late press between GOOD and 200 ms on a long-note head never happens as BAD; the head is already POOR. (**clear**)

---

## 2. Judge windows

### 2.1 Table (ms, symmetric, same for keys and scratch)

| `#RANK` | PGREAT | GREAT | GOOD | BAD | empty POOR |
|---|---|---|---|---|---|
| 0 (VERY HARD) | ±8 | ±24 | ±40 | ±200 | 200 < early < 1000 |
| 1 (HARD) | ±15 | ±30 | ±60 | ±200 | same |
| 2 (NORMAL) | ±18 | ±40 | ±100 | ±200 | same |
| 3 (EASY) | ±21 | ±60 | ±120 | ±200 | same |
| any other value | as NORMAL | | | | |

(**clear**)

### 2.2 How `#RANK` is read

> *Corrected by LR2_COURSES.md*, "Changes to LR2_BEHAVIOUR.md", row 2.2.

- A missing `#RANK` gives NORMAL. (**clear**)
- The header must be `#RANK` followed by a space (tabs are treated as spaces). The value is read as a C-style integer:
  leading digits only, so `2.5` gives 2, `3abc` gives 3, `-1` gives −1, and **a value with no leading digits (or an
  empty value) gives 0, i.e. VERY HARD**. (**clear** for the parse; that a non-numeric value plays VERY HARD is
  **inferred** from the standard behaviour of that conversion.)
- Every value other than 0, 1 and 3 (2, 4, 5, −1, 100, …) plays NORMAL. In particular `#RANK 4` (VERY EASY) plays
  NORMAL. (**clear**)
- The last `#RANK` line that is active (inside a taken `#IF` branch of `#RANDOM`) wins. (**clear**)
- `#DEFEXRANK` and `#EXRANK` are not read at all. (**clear**)
- Inside a NONSTOP (connected) course, a `#RANK` of a later stage changes the windows when that stage begins, but with
  a different table: 0 → 12/24/60/200, 1 → 15/30/80/200, 3 → 21/60/120/200, other → 18/40/100/200.
  (**clear** in the reading; **ambiguous** whether this is intended, since the first stage uses the normal table.)

### 2.3 Options that override the windows

Two "maniac" judge options replace the table: one forces 8/24/40/200, the other 8/12/12/200 (GREAT and GOOD both
12 ms). (**clear**; option names not established.)

### 2.4 Scaling

Windows do not scale with BPM, frame rate, hi-speed, or key mode. (**clear**) The effective windows still widen or
shift with tick granularity (section 0).

---

## 3. Gauges

LR2 has six gauge types: GROOVE (normal), EASY, HARD (survival), HAZARD (DEATH), P-ATTACK and G-ATTACK. There is no
ASSIST EASY, EX-HARD, EX-CLASS or EXHARD-CLASS. (**clear**)

### 3.1 Common rules

> *Corrected by LR2_COURSES.md*, "Changes to LR2_BEHAVIOUR.md", row 3.1.

| rule | confidence |
|---|---|
| The gauge is a real number from 0 to 100. It is **shown** (number and bar) as the integer part rounded down to an even number: shown = 2 × floor(floor(HP) / 2). | clear |
| After every change the gauge is clamped to at most 100 and at least 0. | clear |
| GROOVE and EASY, outside courses: after a change, if the gauge is at or below 2 it is set to 2 (it can never empty). | clear |
| **Low-gauge rule:** before applying a BAD, POOR or empty-POOR change, if the shown gauge is 30 or less (i.e. HP < 32) the change is multiplied by 0.6. It applies to HARD, HAZARD, P-ATTACK and G-ATTACK, and to **every** gauge inside a course (GROOVE and EASY included). It does not apply to GROOVE or EASY outside courses, to gains, or to mines. | clear |
| **Death:** if the gauge is below 2 (shown 0) the play ends at once as FAILED (single play, no battle). A gauge that was below 2 before a judgement is forced to 0 afterwards (no revival). Exactly 2.0 is alive. | clear |
| Start value: 20 for GROOVE and EASY outside courses; 100 for every other gauge, and for every gauge in a course. | clear |
| Clear threshold (end of song, all notes judged): GROOVE, EASY and HAZARD need HP ≥ 80 (the real value: 79.9 fails although it shows 78); HARD, P-ATTACK, G-ATTACK need HP ≥ 2. | clear |

### 3.2 Per-judgement change, single play (percentage points; T = TOTAL, n = note count, f = HARD factor 3.4)

> *Corrected by LR2_COURSES.md*, "Changes to LR2_BEHAVIOUR.md", row 3.2.

| gauge | PGREAT | GREAT | GOOD | BAD | POOR | empty POOR |
|---|---|---|---|---|---|---|
| GROOVE | +T/n | +T/n | +T/(2n) | −4 | −6 | −2 |
| EASY | +1.2·T/n | +1.2·T/n | +1.2·T/(2n) | −3.2 | −4.8 | −1.6 |
| HARD | +0.1 | +0.1 | +0.05 | −6·f | −10·f | −2·f |
| HAZARD (DEATH) | 0 | 0 | 0 | −100 | −100 | 0 |
| P-ATTACK | +0.1 | −1 | −100 | −100 | −100 | −100 |
| G-ATTACK | −10·f | −1 | +0.1 | −6 | −10·f | −2·f |

(**clear** for the table.) The low-gauge rule (×0.6 at shown ≤ 30) multiplies the BAD, POOR and empty-POOR columns
of HARD, HAZARD, P-ATTACK and G-ATTACK, not the others. Note that G-ATTACK's −10·f on PGREAT is never reduced (it is
not a BAD/POOR).

Precision note (**ambiguous**): the GROOVE/EASY gain T/n appears to be accumulated in single precision. When an
exact sum would land exactly on an even shown value (e.g. 20 + 50 × 0.96 = 68), the stored value is a hair below and
the gauge shows 2 % less (66). Whether the original binary does the same is not established.

### 3.3 TOTAL

- `#TOTAL` must be followed by a space; the value is read as an integer (fraction dropped: `#TOTAL 250.9` is 250).
  (**clear**)
- If `#TOTAL` is missing, or its integer value is **less than 1** (0 or negative), the default is
  TOTAL = floor(0.8 × B), where B = 200 + n/5 for n < 400; B = 280 + (n − 400)/2.5 for 400 ≤ n < 600;
  B = 360 + (n − 600)/5 for n ≥ 600. Equivalently TOTAL = floor(160 + 0.16 × (n + clamp(n − 400, 0, 200))), rounded
  **down to a whole number**. Examples: n = 200 → 192, 500 → 256, 1000 → 352, 3 → 160, 7 → 161. (**clear**)
- n here is the note count of 3.5 (mines excluded, long note = 1).
- TOTAL has no upper clamp and no lower clamp other than the default rule.

### 3.4 HARD damage factor f (used by HARD, and by parts of G-ATTACK)

f = max(g(n), h(TOTAL)).

**TOTAL part.** Let r = floor((TOTAL − 80) / 16) (truncated toward zero); if r ≤ 0 use r = 1. Then h = 10 / r.

| TOTAL | r | h(TOTAL) |
|---|---|---|
| below 112 | 1 | 10 |
| 112 – 127 | 2 | 5 |
| 128 – 143 | 3 | 3.333… |
| 144 – 159 | 4 | 2.5 |
| 160 – 175 | 5 | 2 |
| 176 – 191 | 6 | 1.666… |
| 192 – 207 | 7 | 1.428 6 (e.g. TOTAL 200) |
| 208 – 223 | 8 | 1.25 |
| 224 – 239 | 9 | 1.111… |
| 240 – 255 | 10 | 1 |
| 256 and up | ≥ 11 | below 1 (so g decides) |

(**clear** in the reading, but flagged there as uncertain; **ambiguous** at TOTAL 192–207: S1 measured a
POOR damage of 15 at TOTAL 200, this formula gives 14.29. At every other multiple of 10 that S1 measured, this
formula reproduces S1's value. The steps are every 16 from 80, not every 10.)

**Note-count part** g(n):

| n | g(n) |
|---|---|
| below 20 | 10 |
| 20 – 29 | 10 − 0.2 × (n − 20)  (20 → 10, 29 → 8.2) |
| 30 – 44 | 7 − (n − 30)/15  (30 → 7, 44 → 6.067) |
| 45 – 59 | 6 − (n − 45)/15 |
| 60 – 124 | 5 − (n − 60)/65  (124 → 4.015) |
| 125 – 249 | 4 − (n − 125)/125 |
| 250 – 499 | 3 − (n − 250)/250 |
| 500 – 999 | 2 − (n − 500)/500 |
| 1000 and up | 1 |

(**clear** for 30 notes and up. **ambiguous** for 20–29: the formula jumps from 8.2 at 29 notes to 7 at 30 notes, the
only discontinuity in the table, and the reading flags it as uncertain; a continuous version would be
10 − 0.3 × (n − 20). The measurement kit's A4 charts with 29 and 30 notes settle it.)

So HARD POOR damage = 10 × f, BAD = 6 × f, empty POOR = 2 × f, each × 0.6 when shown gauge ≤ 30.

### 3.5 HAZARD (DEATH)

No gain from any judgement; BAD or POOR −100 (instant death, also under the ×0.6 rule since 60 > any gauge ≤ 30);
empty POOR costs nothing; mines do take their damage. Starts at 100. (**clear**) Its clear rule is GROOVE's (HP ≥ 80
at the end), so a HAZARD survivor that took mine damage below 80 without a full combo would fail; in practice
surviving HAZARD means no BAD/POOR, which is a full combo anyway (7).

### 3.6 Courses (class / dan and others)

> *Corrected by LR2_COURSES.md*, "Changes to LR2_BEHAVIOUR.md", row 3.6.

Courses have three kinds: expert (free) courses, NONSTOP (connected) courses and class (grade, 段位) courses.
In all of them every gauge starts at 100, the gauge carries over from stage to stage, the ×0.6 low-gauge rule applies
to every gauge, there is **no** 2 % floor for judgements (GROOVE and EASY die below 2 too), and the course is failed as
soon as the gauge is below 2. (**clear**)

| course kind | gauge selected | PG | GR | GD | BAD | POOR | empty POOR |
|---|---|---|---|---|---|---|---|
| class | GROOVE or EASY | +0.1 | +0.1 | +0.04 | −2 | −3 | −2 |
| class | HARD | +0.1 | +0.1 | +0.04 | −6·f | −10·f | −2·f |
| expert / NONSTOP | GROOVE | +0.1 | +0.1 | +0.04 | −1.5 | −2 | −1.5 |
| expert / NONSTOP | EASY | +0.12 | +0.12 | +0.048 | −1.2 | −1.6 | −1.2 |
| expert / NONSTOP | HARD | +0.1 | +0.1 | +0.05 | −6·f | −10·f | −2·f |
| any | HAZARD, P-ATTACK, G-ATTACK | as single play | | | | | |

(**clear**.) f is computed per stage from that stage's TOTAL and notes (in a NONSTOP course, from all stages'
notes together and the last `#TOTAL` read: **inferred**). Course clear: FAILED if the gauge ended below 2 or the
course was not finished; otherwise FULL COMBO if the course combo equals the course's total notes; otherwise HARD
CLEAR for HARD/P-ATTACK/G-ATTACK, EASY CLEAR for EASY, CLEAR for the others, each needing HP **strictly above** 2.
(**clear**)

Mines in courses still stop at 2 % on GROOVE/EASY (the floor in the mine rule ignores the course flag). (**inferred**,
looks like an oversight in the original.)

---

## 4. Mines

> *Corrected by LR2_COURSES.md*, "Changes to LR2_BEHAVIOUR.md", row 4.

| rule | confidence |
|---|---|
| Mines are the objects on channels D1–D9 (1P) and E1–E9 (2P). They sit in the lane's queue like notes but are never counted as notes and never counted in TOTAL's n. | clear |
| Damage = the object's two-character value read in base 36, taken directly as percentage points: `04` = 4, `0K` = 20, `1E` = 50, `ZZ` = 1295. The gauge loses exactly that much. Not scaled by gauge type, TOTAL, note count or the ×0.6 rule; the same on every gauge. | clear |
| After the damage: GROOVE/EASY stop at 2 (so `ZZ` takes them to 2 %); any other gauge below 2 dies (FAILED). | clear |
| Trigger 1 (press): a **new press** on the mine's lane while the mine is under that lane's cursor and less than the **GREAT** window ahead of its time (24/30/40/60 ms by `#RANK`), or already past it on the same tick. The mine explodes at once. | clear |
| Trigger 2 (held): on the first tick after the mine's time, if the lane is newly pressed or held, the mine explodes. | clear |
| Either way the mine is removed from the lane on the first tick after its time (or when it explodes). A release on that tick, or a late tap after that tick, does not set it off. There is no late press window. | clear |
| While a mine is under the cursor and more than the GREAT window ahead, a press on that lane does nothing (no empty POOR, no keysound). | clear |
| An exploding mine adds 1 to the POOR count, so it counts in BP. It does not change the combo, the EX score, the 200 000-point score, the judged-note count, or the FULL COMBO test. | clear for POOR +1 and no combo change |
| A mine may show as **2** POORs on the play/result screen while BP and the saved POOR count rise by 1 (a replay shows 1). | ambiguous (likely a decompilation artefact) |
| In a NONSTOP course, a mine of the 2nd and later stage gets 1296 × (stage index) added to its value (an instant fail). | ambiguous (artefact of how course objects are numbered) |

---

## 5. Long notes

### 5.1 Which LNs exist

| rule | confidence |
|---|---|
| LN channels 51–59 / 61–69: in each channel the objects pair up in order; the first of a pair is the head, the second the tail. | clear |
| `#LNOBJ xx`: an object in a normal note channel whose value is xx ends a long note whose head is the previous note in that channel; the end object is not a note. | clear |
| `#LNTYPE` is not read (only the pairing above, i.e. `#LNTYPE 1` behaviour). | clear |
| There is only one LN kind: no CN, no HCN, no separate tail judgement, no LN-mode option. | clear |
| An LN counts as one note for n, combo, EX and score. | clear |
| When an LN head and a normal note share a lane and a time, the LN replaces the note. Objects that start inside an LN's span (after its head, up to and including its tail) in the same lane are dropped and not counted. Two notes at the same time in one lane count once. | clear |

### 5.2 Judging an LN

1. **Miss:** if the LN has not been started and the song time is more than the **GOOD** window past the head, the LN is
   POOR (one POOR, combo broken) and is removed. (**clear**)
2. **Press on the head**, d = |press − head|: d ≤ PG → pending PGREAT; ≤ GR → pending GREAT; ≤ GD → pending GOOD;
   ≤ 200 (only possible early, see step 1) → pending BAD. The LN is now "held". **Nothing is recorded yet**: no
   combo, no gauge change, no count. More than 200 ms and less than 1000 ms early → empty POOR, LN not started.
   (**clear**)
3. **Held to the end:** on the first tick after the tail time with the lane still held, the pending judgement is
   recorded (combo, gauge, counts all change **at the tail**, not at the head). (**clear**)
4. **Release before the end:** if the release comes more than the GOOD window before the tail
   (tail − release > GOOD), the LN is a **BAD** (combo broken, BAD gauge change) whatever the head was, and its sound
   stops. If it comes within the GOOD window before the tail, the pending head judgement is recorded. There is no
   "too early = POOR" limit and no late-release window. (**clear**)
5. A head pressed early in the BAD range stays a BAD whether held or released. (**clear**)
6. Holding the key through the head without a fresh press does not start the LN; it becomes POOR at GOOD-late.
   (**clear**)
7. While an LN is held, a **new press** on the same lane (in practice: reversing the turntable, or the other scratch
   key) re-judges the head: if the head is still within a window, the pending judgement is replaced; otherwise it is
   an **empty POOR** (gauge damage and +1 POOR, LN continues). (**inferred**: follows from the order of checks.)

Release window by `#RANK`: VERY HARD 40, HARD 60, NORMAL 100, EASY 120 ms before the tail.

---

## 6. Combo and BP

> *Corrected by LR2_COURSES.md*, "Changes to LR2_BEHAVIOUR.md", row 6.

| rule | confidence |
|---|---|
| PGREAT, GREAT, GOOD: combo +1 (an LN at its tail). | clear |
| BAD, POOR (including an LN early release BAD and an LN miss POOR): combo to 0. | clear |
| Empty POOR, mine: combo unchanged. | clear |
| Max combo is the highest combo reached. A course also keeps a separate running course combo. | clear |
| POOR count = POORs of notes + empty POORs + exploded mines. | clear |
| BP (miss count) = BAD + POOR count, so it includes empty POORs and mines. For a play that did not judge all notes (failed or quit) the saved best BP adds the notes never judged. | clear |

---

## 7. Score, EX score, rank and clear lamp

### 7.1 Scores

| quantity | rule | confidence |
|---|---|---|
| EX score | 2 × PGREAT + GREAT | clear |
| score (max 200 000) | floor(50 000 × (4·PGREAT + 2·GREAT + GOOD) / n) | clear |
| DJ level | k = floor(9 × EX / (2n)), capped at 8, and raised to 1 if EX > 0. 8 = AAA (EX ≥ 8/9 of max), 7 = AA, 6 = A, 5 = B, 4 = C, 3 = D, 2 = E, 1 or 0 = F. | clear |
| best score saved | a play replaces the stored best only when its EX score is strictly higher. | clear |

### 7.2 Clear lamp

> *Corrected by LR2_COURSES.md*, "Changes to LR2_BEHAVIOUR.md", row 3.6 / 7.2.

LR2 has five lamps: FAILED < EASY CLEAR < CLEAR (normal) < HARD CLEAR < FULL COMBO. There is no PERFECT, MAX,
ASSIST or LIGHT ASSIST lamp. (**clear**)

Order of decision for a single play (**clear**):

1. If max combo equals the note count → **FULL COMBO**, on any gauge and **whatever the final gauge** (a GROOVE play
   that full-combos with many empty POORs or mines and ends at 40 % is a FULL COMBO). Empty POORs and mines never
   break it. (Edge: a chart with 0 notes satisfies this.)
2. Else, HARD, P-ATTACK, G-ATTACK: all notes judged and gauge ≥ 2 → HARD CLEAR; otherwise FAILED.
3. Else (GROOVE, EASY, HAZARD): all notes judged and gauge ≥ 80 → CLEAR for GROOVE and HAZARD, EASY CLEAR for EASY;
   otherwise FAILED.
4. A play that ends with notes still unjudged (quit) has its gauge set to 0 → FAILED.

Lamp caps applied afterwards (**inferred** for the option names, which are option indices in the reading):

- EASY gauge: lamp at most EASY CLEAR, except a FULL COMBO stays FULL COMBO.
- AUTO-SCRATCH (assist), the highest random option indices (4 and 5, taken to be H-RANDOM and ALL-SCRATCH) and the
  constant-hi-speed option on a chart that changes BPM: lamp at most EASY CLEAR (a FULL COMBO is cut down too), and the
  play keeps only the lamp and play counts, no EX/score/BP best.
- A clear lamp is only ever raised, never lowered, by later plays.

---

## 8. Other rules a faithful LR2-equivalent player must match

| rule | confidence |
|---|---|
| Autoplay judges every note PGREAT at its time (an LN at its tail) and the gauge moves accordingly. If an IR rival/target ghost is loaded, autoplay instead replays that ghost's judgements. | clear (the ghost case: inferred) |
| Notes in a lane are resolved strictly in order; a mine blocks a lane's cursor until it passes. | clear |
| Key mode makes no difference to any rule above (5, 7, 9/PMS, 10, 14 keys all the same). | clear |
| Negative BPM (backward scroll) mirrors the judging clock around the time of the change. | inferred |
| The judge windows are integers and the comparison is on whole milliseconds; the note time's fraction is dropped for window tests, but kept for the 200 ms (normal) / GOOD (LN) miss test. | clear |
| HARD gauge gain at full gauge is lost (clamped at 100). | clear |
| In battle / ghost battle, the immediate-fail rule waits until both sides are below 2. | clear |

---

## Verification against the measurement kit

Predictions of what LR2 should show if it behaves as OpenLR2 does. Counts are per 16 trials.

### A11

| chart | named counts (kit) | prediction | extra discriminator |
|---|---|---|---|
| `a11-1` (VH, 120 ms apart) | BAD ≈ 16 | first key-1 note 120 ms late → BAD; the BAD chain then judges the second key-1 note with the same press (≈ 0 ms) → PGREAT/GREAT | POOR ≈ 0. Pure "earliest note" without the chain would give POOR ≈ 16. |
| `a11-2` (EASY, 90 ms apart) | GOOD ≈ 16 | first note 90 ms late → GOOD (≤ 120); no chain after a GOOD; the second note is never pressed → POOR | POOR ≈ 16 |
| `a11-3` (VH scratch, 80 ms apart) | BAD / POOR ≈ 32 / 0 | first press: first note 160 ms late → BAD; chain: second note 80 ms late → BAD (> 40). Second press: the next scratch note is a trial away (≈ 2.5 s) → nothing | — |

This is the kit's first row ("always the earliest note in reach"). The kit's action for that row is to make the
profile's choice `Lowest`; the BAD chain must be added as well, or `a11-1`'s POOR count will differ (16 vs 0).

### A3

| `#RANK` | L = 70 | L = 110 |
|---|---|---|
| 0 | BAD | BAD |
| 2 | GOOD | BAD |
| 3 | GOOD | GOOD |
| 4, 5, −1 | GOOD | BAD (same as 2) |

A3 closes for 4, 5 and −1 (NORMAL), as vibeBMS does.

### A4 (HARD, TOTAL 300 → TOTAL factor 0.77, so g(n) decides; three empty POORs of 2 × g(n) each, from 100)

| notes | 20 | 29 | 30 | 31 | 45 | 60 | 90 | 110 | 124 | 125 |
|---|---|---|---|---|---|---|---|---|---|---|
| gauge (real) | 40 | 50.8 | 58 | 58.4 | 64 | 70 | 72.8 | 74.6 | 75.9 | 76 |
| shown | **40** | **50** | **58** | **58** | **64** | **70** | **72** | **74** | **74** | **76** |

Identical to the kit's vibeBMS row. If LR2 shows 56 at 29 notes instead of 50, the 20–29 segment is continuous
(10 − 0.3 × (n − 20)) and OpenLR2's 20–29 formula is a decompilation artefact. Caution for the steps: each tap must
come more than 200 ms and **less than 1000 ms** before the key-1 note; a tap a second or more early does nothing.

### A7 (GROOVE, autoplay)

Default TOTAL 192, 256, 352, as the kit's vibeBMS rows. Predicted readings equal those rows, with one possible
exception: `a7-n200` after group 5 may read **66** instead of 68 if LR2, like the rewrite, adds the gain in single
precision (ambiguous, see 3.2). Run autoplay without an IR rival target loaded, otherwise autoplay replays the
target's judgements.

### A8 (`#TOTAL 1000`, 8 notes → each PGREAT +125 on GROOVE)

| run | after mine 4 | 20 | 50 | `ZZ` | lamp / counts |
|---|---|---|---|---|---|
| 1 GROOVE, hold | 96 | 76 | 26 | 2 | **FULL COMBO** (max combo 8 = notes) although the gauge ends at 2 %; POOR 4 in BP (the screen may show 8, see 4) |
| 2 GROOVE, tap | 96 / 76 / 26 / 2 for each mine that goes off | | | | a tap sets a mine off only if it lands between the GREAT window early (24–60 ms by `#RANK`) and the first tick after the mine; late taps do nothing |
| 3 HARD, hold | 96 | 76 | 26 | 0 → FAILED | the same damage on HARD (no ×0.6, no factor); it fails only at `ZZ` |

This matches the kit's "value as percent" row; S10's GROOVE readings (−8, −8, −10, −12, −16) do not fit OpenLR2.

---

## Not determined

- Whether the original binary computes the TOTAL factor exactly as "steps of 16 from 80" (flagged as uncertain; S1's 15 at TOTAL 200 disagrees by one step's rounding).
- Whether the 20–29-note segment of g(n) really jumps from 8.2 to 7 at 30 notes.
- Whether a mine shows as one or two POORs on screen.
- Single- vs double-precision gauge gains (affects only exact even boundaries).
- The menu names behind random option indices 4 and 5 and the hi-speed option that cap the lamp.
- OpenLR2's GitHub issues and discussions were not read; its README was read.
