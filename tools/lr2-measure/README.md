# lr2-measure

Test charts and steps for a player with Lunatic Rave 2 to settle the LR2 equivalence gaps vibeBMS cannot close
from public documents (`docs/RULES.md`, "LR2 equivalence"). Everything here is measured by playing LR2 and reading
its screen: no decompiling, no other player's code. Send back the numbers each section asks for; the tables say
what vibeBMS does with them.

The main one is **A11**, the note a press takes when two notes of a lane are both in reach. It reaches every play,
so until it is measured no vibe-LR2 play goes to Bokutachi.

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

| `a11-1` BAD | `a11-2` GOOD | `a11-3` BAD / POOR | LR2's rule | what vibeBMS does |
|---|---|---|---|---|
| about 16 | about 16 | about 32 / 0 | always the earliest note in reach | make the profile's choice `Lowest`; close A11 |
| about 0 | about 16 | about 32 / 0 | the note a GOOD or better can take; when both or neither can, the earlier (beatoraja's "combo" as ralba-gear describes it) | add that rule (vibeBMS's Combo but taking the later note only when it is itself within GOOD), make it the profile's; close A11 |
| about 0 | about 16 | about 16 / 16 | vibeBMS's Combo: the later note once the earlier is late beyond GOOD and the later is not early beyond GOOD | nothing to change; close A11 |
| about 0 | about 0 | about 16 / 16 | the nearest note | make the profile's choice `Duration`; close A11 |
| anything else | | | none of these | send the counts; A11 stays open |

("About" leaves room for a few trials off: 12 of 16 or more decides. In `a11-2` a guide note judged GOOD also
counts; play it again if the guide notes show more than a couple of GOODs.)

## A3: what LR2 does with `#RANK 4`, 5 and -1

`a3-rankR-L.bme` (R = 0, 2, 3, 4, 5, m1 for -1; L = 70 or 110): a key-1 note, and a key-7 note L ms after it.
Press key 1 + key 7 together on the key-7 note, so the key-1 note is hit L ms late. Play all twelve.

`#RANK` 0, 2 and 3 are the references (VERY HARD, NORMAL, EASY; their windows are measured). Compare the GREAT /
GOOD / BAD counts of 4, 5 and -1 with them, at both lateness: the reference they match is what LR2 plays them as.
vibeBMS plays all three as NORMAL; if LR2 matches `#RANK 2` too, A3 closes for that value.

## A4: HARD damage on charts of few notes

`a4-nN.bme` (N = 20, 29, 30, 31, 45, 60, 90, 110, 124, 125 notes, `#TOTAL 300`), **HARD** gauge: hit the key-7
notes (any BAD or POOR spoils the run: play it again), then, while the single key-1 note at the end is still more
than 0.2 s from the line, tap key 1 three times (three empty POORs). Read the gauge in the pause before the note
reaches the line. Each empty POOR costs 2 % × the note-count factor.

| notes | 20 | 29 | 30 | 31 | 45 | 60 | 90 | 110 | 124 | 125 |
|---|---|---|---|---|---|---|---|---|---|---|
| vibeBMS | 40 | 50 | 58 | 58 | 64 | 70 | 72 | 74 | 74 | 76 |
| S1's slopes doubled at each halving | 58 | 62 | 62 | 62 | 66 | 68 | 72 | 74 | 74 | 76 |

125 is the reference (S1's graph). If LR2 reads vibeBMS's row, A4 closes; if the second row, vibeBMS takes that
rule below 125 notes and A4 closes; otherwise send the readings.

## A7: the TOTAL of a chart without `#TOTAL`

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

vibeBMS's TOTAL is 160 + (N + clamp(N − 400, 0, 200)) × 0.16 (LR2oraja's README says it is LR2's). If LR2 reads
vibeBMS's rows, A7 closes. Otherwise each reading R after group g gives LR2's TOTAL as about (R − 20) × N / (10 g),
to within 2 × N / (10 g): send the readings.

## A8: mines

`a8-mines.bme` (`#TOTAL 1000`): eight key-7 notes fill the gauge, then four mines in lane 1, one every 2 measures,
values 4, 20, 50 and `ZZ`. Lane 1 has no notes.

1. GROOVE: hit the key-7 notes; in the rest after them press key 1 and **hold it to the end**. Read the gauge after
   each mine.
2. GROOVE again, but **tap** key 1 as each mine reaches the line (and let go), instead of holding.
3. HARD, holding as in 1.

| gauge after the mines (GROOVE, from 100) | 4 | 20 | 50 |
|---|---|---|---|
| value as percent (vibeBMS; S14) | 96 | 76 | 26 |
| value / 2 percent (S9, S15: nanasi's) | 98 | 88 | 62 |
| value as percent + 6 (fits S10's readings) | 90 | 64 | 8 |

After `ZZ`: S14 says GROOVE goes to 2 % and a survival gauge fails. Run 2 tells whether a tap inside the GOOD
window sets a mine off (S14 says it does; vibeBMS sets it off only when the key is down as the mine passes). Run 3
tells whether HARD takes the same damage (S14) or fails at the first mine (S10). If all three match vibeBMS, A8
closes; otherwise send the readings.
