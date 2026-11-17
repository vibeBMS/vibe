# lr2-measure

Test charts and steps for a player with Lunatic Rave 2 to check vibeBMS's LR2 rules (`docs/RULES.md`, "LR2
equivalence"). Everything here is measured by playing LR2 and reading its screen: no decompiling, no other
player's code. Send back the numbers each section asks for; the tables say what vibeBMS does with them.

vibeBMS's rows are the predictions of S-OLR2 (`docs/LR2_BEHAVIOUR.md`, a behaviour specification written from a
reading of a decompilation-based rewrite of LR2, not a measurement). Most of the gaps they settled are closed on
its word; the readings here confirm it or reopen them. Two are still open: **A4** at 21 to 29 notes (the
`a4-n29` and `a4-n30` charts), and the TOTAL factor at 192 .. 207, which S1 measured as 15 and S-OLR2 gives as
14.29 (`a4-t200`).

`charts/` is made by `gen.py` (`python3 gen.py` writes it again and prints the tables below). The test
`crates/vibebms-core/tests/lr2_measure.rs` plays the same charts in vibeBMS the way these steps say and checks
vibeBMS's rows of the tables.

## Before you start

- LR2 (100201). Copy the `charts` folder into a song folder LR2 reads and reload the song list. The charts show up
  with genre `LR2 MEASURE`.
- 1P side, no lane option (OFF), no AUTO SCRATCH, no assist. Any HI-SPEED you can read.
- LR2 SETUP: vertical sync **off** (LR2's windows are only its documented ones with it off). JUDGE AUTO ADJUST off;
  your usual JUDGE TIMING is fine.
- Gauge: GROOVE unless a section says otherwise.
- "Key 1" .. "key 7" are the seven keys left to right; "scratch" is the turntable (on a keyboard, either scratch
  key).
- After each play, write down the result screen's PGREAT, GREAT, GOOD, BAD, POOR counts (a screenshot is best).
  A trial you know you botched (a press far off) is fine; the tables allow a few.

## A11: which note a press takes

Each chart repeats one situation 16 times, one trial every 2 measures. Each trial has a **guide note** in another
lane: press the keys named together with it, as a chord, as on time as you can. Do not press anything else. The
guide notes add their own PGREAT / GREAT (and a GOOD now and then) to the counts; the tables only use the counts
named.

| chart | `#RANK` | the trial | press |
|---|---|---|---|
| `a11-1.bme` | 0 (VERY HARD) | two key-1 notes 120 ms apart; a key-7 note with the second | key 1 + key 7 on the key-7 note |
| `a11-2.bme` | 3 (EASY) | two key-1 notes 90 ms apart; a key-7 note with the second | key 1 + key 7 on the key-7 note |
| `a11-3.bme` | 0 (VERY HARD) | two scratch notes 80 ms apart; a key-6 note 80 ms after the second, a key-7 note 80 ms after that | scratch + key 6 on the key-6 note, then scratch again (the other way, or the other scratch key) + key 7 on the key-7 note |

`a11-3` is a quick roll (about two 1/16 notes at 190 BPM). The first press finds both scratch notes late beyond
GOOD; the second shows whether the one the first press left is still in reach.

What each candidate rule gives (16 trials):

| `a11-1` BAD / POOR | `a11-2` GOOD / POOR | `a11-3` BAD / POOR | LR2's rule | what vibeBMS does |
|---|---|---|---|---|
| about 16 / 0 | about 16 / 16 | about 32 / 0 | **vibeBMS, S-OLR2**: always the earliest note; after a BAD the same press also takes the next note within 200 ms (the BAD chain): `a11-1`'s second key-1 note is a PGREAT, GREAT or GOOD | nothing to change; A11 stays closed |
| about 16 / 16 | about 16 / 16 | about 32 / 0 | always the earliest note, no BAD chain | drop the chain; A11 stays closed |
| about 0 / 16 | about 16 / 16 | about 32 / 0 | the note a GOOD or better can take; when both or neither can, the earlier (beatoraja's "combo" as ralba-gear describes it) | reopen A11; send the counts |
| about 0 / 16 | about 16 / 16 | about 16 / 16 | the later note once the earlier is late beyond GOOD (vibeBMS's Combo) | reopen A11; send the counts |
| about 0 / 16 | about 0 / 16 | about 16 / 16 | the nearest note | reopen A11; send the counts |
| anything else | | | none of these | reopen A11; send the counts |

("About" leaves room for a few trials off: 12 of 16 or more decides. In `a11-2` a guide note judged GOOD also
counts; play it again if the guide notes show more than a couple of GOODs.)

## A3: what LR2 does with `#RANK 4`, 5 and -1 (closed on S-OLR2's word)

`a3-rankR-L.bme` (R = 0, 2, 3, 4, 5, m1 for -1; L = 70 or 110): a key-1 note, and a key-7 note L ms after it.
Press key 1 + key 7 together on the key-7 note, so the key-1 note is hit L ms late. Play all twelve.

`#RANK` 0, 2 and 3 are the references (VERY HARD, NORMAL, EASY; their windows are measured). Compare the GREAT /
GOOD / BAD counts of 4, 5 and -1 with them, at both lateness: the reference they match is what LR2 plays them as.
vibeBMS plays all three as NORMAL, as S-OLR2 says LR2 does: at 70 ms `#RANK` 0 BAD, 2 GOOD, 3 GOOD; at 110 ms 0
BAD, 2 BAD, 3 GOOD; 4, 5 and -1 as 2. If one does not match `#RANK 2`, A3 reopens for that value.

## A4: HARD damage on charts of few notes

`a4-nN.bme` (N = 20, 29, 30, 31, 45, 60, 90, 110, 124, 125 notes, `#TOTAL 300`), **HARD** gauge: hit the key-7
notes (any BAD or POOR spoils the run: play it again), then, while the single key-1 note at the end is more than
0.2 s and less than 1 s from the line, tap key 1 three times (three empty POORs; a tap a second or more early does
nothing). Read the gauge in the pause before the note reaches the line. Each empty POOR costs 2 % × the
note-count factor.

| notes | 20 | 29 | 30 | 31 | 45 | 60 | 90 | 110 | 124 | 125 |
|---|---|---|---|---|---|---|---|---|---|---|
| vibeBMS, S-OLR2 | 40 | 50 | 58 | 58 | 64 | 70 | 72 | 74 | 74 | 76 |
| 21 to 29 notes continuous (10 − 0.3 a note above 20) | 40 | 56 | 58 | 58 | 64 | 70 | 72 | 74 | 74 | 76 |
| S1's slopes doubled at each halving | 58 | 62 | 62 | 62 | 66 | 68 | 72 | 74 | 74 | 76 |

125 is the reference (S1's graph). If LR2 reads vibeBMS's row, A4 closes; if 56 at 29 notes, vibeBMS takes the
continuous segment and A4 closes; otherwise send the readings.

### The TOTAL factor at 200

`a4-t200.bme` (`#TOTAL 200`, 1000 notes, so the note-count factor is 1), **HARD** gauge: hit the 498 chords of key
3 + key 5 (any BAD or POOR spoils the run), then let the four key-7 notes that follow pass without pressing (four
POORs, one every 2 measures). Read the gauge after each.

| after POOR | 1 | 2 | 3 | 4 |
|---|---|---|---|---|
| vibeBMS, S-OLR2 (10 / floor((200 − 80) / 16) = 1.43: POOR −14.29) | 84 | 70 | 56 | 42 |
| S1's 15 at TOTAL 200 (1.5) | 84 | 70 | 54 | 40 |

S1 read one POOR at TOTAL 200 as 15; one POOR shows 84 under both, the third and fourth tell them apart. If LR2
reads the second row, vibeBMS takes 1.5 from 192 to 207.

## A7: the TOTAL of a chart without `#TOTAL` (closed on S-OLR2's word)

`a7-nN.bme` (N = 200, 500, 1000 notes, no `#TOTAL`), GROOVE gauge, **autoplay**: notes come in groups of 10 with a
rest after each; read the gauge in each rest until it is full (100 %).

| chart | after group 1, 2, 3, … |
|---|---|
| `a7-n200`, vibeBMS (TOTAL 192) | 28 38 48 58 68 76 86 96 100 |
| `a7-n200`, TOTAL 160 | 28 36 44 52 60 68 76 84 92 100 |
| `a7-n500`, vibeBMS (TOTAL 256) | 24 30 34 40 44 50 54 60 66 70 76 80 86 90 96 100 |
| `a7-n500`, TOTAL 160 | 22 26 28 32 36 38 42 44 48 52 54 58 60 64 68 70 74 76 80 84 86 90 92 96 100 |
| `a7-n1000`, vibeBMS (TOTAL 352) | 22 26 30 34 36 40 44 48 50 54 58 62 64 68 72 76 78 82 86 90 92 96 100 |
| `a7-n1000`, TOTAL 160 | 20 22 24 26 28 28 30 32 34 36 36 38 40 42 44 44 46 48 50 52 52 54 56 58 60 (25 groups) |

vibeBMS's TOTAL is floor(160 + (N + clamp(N − 400, 0, 200)) × 0.16), as S-OLR2 gives LR2's (LR2oraja's README
gives the formula). If LR2 reads otherwise, A7 reopens: each reading R after group g gives LR2's TOTAL as about
(R − 20) × N / (10 g), to within 2 × N / (10 g); send the readings. Run autoplay without an IR rival target
loaded (with one, LR2's autoplay replays the target's judgements). S-OLR2 notes that `a7-n200` may read 66
instead of 68 after group 5 if LR2 adds the gain in single precision.

## A8: mines (closed on S-OLR2's word)

`a8-mines.bme` (`#TOTAL 1000`): eight key-7 notes fill the gauge, then four mines in lane 1, one every 2 measures,
values 4, 20, 50 and `ZZ`. Lane 1 has no notes.

1. GROOVE: hit the key-7 notes; in the rest after them press key 1 and **hold it to the end**. Read the gauge after
   each mine, and the result screen's lamp and POOR count.
2. GROOVE again, but **tap** key 1 as each mine reaches the line (and let go), instead of holding.
3. HARD, holding as in 1.

| gauge after the mines (GROOVE, from 100) | 4 | 20 | 50 | `ZZ` |
|---|---|---|---|---|
| value as percent (vibeBMS, S-OLR2; S14) | 96 | 76 | 26 | 2 |
| value / 2 percent (S9, S15: nanasi's) | 98 | 88 | 62 | 2 |
| value as percent + 6 (fits S10's readings) | 90 | 64 | 8 | 2 |

vibeBMS and S-OLR2, run by run:

| run | after 4, 20, 50, `ZZ` | lamp and counts |
|---|---|---|
| 1 GROOVE, hold | 96 76 26 2 | FULL COMBO (max combo 8 = the notes) though the gauge ends at 2 %; POOR 4, BP 4 (S-OLR2: the screen may show 8) |
| 2 GROOVE, tap | 96, 76, 26, 2 for each mine that goes off | a tap sets a mine off only from the GREAT window before it (this chart is NORMAL: 40 ms) to the moment it passes; a tap more than 40 ms early, or after it, does nothing |
| 3 HARD, hold | 96 76 26 0 | FAILED at `ZZ`: the same damage on HARD, no × 0.6, no factor |

If a run differs, A8 reopens: send the readings. (S14 gives the GOOD window for the tap; S-OLR2 the GREAT window: a
tap 41 to 100 ms early tells them apart.)
