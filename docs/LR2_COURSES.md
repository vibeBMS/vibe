> **Origin:** clean-room behaviour specification written from a reading of OpenLR2 branch LR2Beta3-v100201, a
> decompilation-based rewrite of LR2; facts only, no code. It extends `docs/LR2_BEHAVIOUR.md` (S-OLR2), and its
> table "Changes to LR2_BEHAVIOUR.md" is the one list of corrections to it (marked there where they apply). Kept as
> received below this header. `docs/RULES.md` cites it as source S-OLR2b.

# LR2 (beta3 100201) attack gauges and courses: supplementary behaviour specification

Clean-room behaviour specification written from a reading of OpenLR2, branch `LR2Beta3-v100201`, a rewrite of
Lunatic Rave 2 beta3 100201 from a decompilation. It extends `docs/LR2_BEHAVIOUR.md` (called "the first spec" below,
with its section numbers) and does not repeat it. Where the `main` branch was compared, that is stated; the parts
compared are the attack gauge tables, NONSTOP stage joining, the NONSTOP `#RANK` table, the stage value offset and the
course clear check.

Confidence labels are as in the first spec:

- **clear**: stated plainly and consistently in the reading.
- **inferred**: follows from how two parts interact, or from the meaning of an option index.
- **ambiguous**: flagged as uncertain in the rewrite, or looks like a decompilation artefact, or the reading is
  self-inconsistent.

Terms: "stage" is one chart of a course, counted from 1 for the player. "Join" is the point where a NONSTOP course
passes from one stage to the next. "Bar line" is the start of a measure. Positions "in measures" count whole measures
plus the fraction of the measure, as in the BMS channel data, whatever the measure's length.

---

## 1. P-ATTACK and G-ATTACK

### 1.1 Gauge option order

| rule | confidence |
|---|---|
| The gauge option has six values, cycled by the gauge key one step at a time and wrapping from the last back to the first: GROOVE → HARD → HAZARD → EASY → P-ATTACK → G-ATTACK → GROOVE. The same order is used for the saved option record (gauge index 0–5 in that order). | clear for the order of indices; inferred for which index is P-ATTACK and which G-ATTACK (index 4 rewards only PGREAT, index 5 rewards only GOOD) |
| The text shown for each value is not built in: it comes from the theme's option-string file (its `#GAUGE` line, six entries). | clear |
| 2P has its own gauge option with the same cycle (battle). | clear |

### 1.2 Values (single play, NONSTOP and expert courses; same numbers in all three)

T, n and f are as in the first spec 3.2–3.4. Start value 100 for both gauges, in and out of courses.

| gauge | PGREAT | GREAT | GOOD | BAD | POOR | empty POOR |
|---|---|---|---|---|---|---|
| P-ATTACK | +0.1 | −1 | −100 | −100 | −100 | −100 |
| G-ATTACK | −10·f | −1 | +0.1 | −6 | −10·f | −2·f |

| rule | confidence |
|---|---|
| The table above is the same in single play, in the expert/NONSTOP course table and in the class course table (where G-ATTACK cannot be used, 5.1). The `main` branch has the same values. | clear |
| G-ATTACK's PGREAT damage of −10·f is real in the reading: it appears identically in all three gauge tables, on both branches, and is not flagged as uncertain. It fits the gauge's idea (only GOOD heals; PGREAT costs as much as a POOR). | clear in the reading; that the original binary does the same is inferred |
| G-ATTACK's BAD is a flat −6, not scaled by f. | clear |
| Low-gauge rule (first spec 3.1) applies to both: at shown gauge ≤ 30 the BAD, POOR and empty-POOR changes are × 0.6. It never touches PGREAT, GREAT or GOOD, so G-ATTACK's PGREAT −10·f and P-ATTACK's GOOD −100 are never reduced. | clear |
| Mines take their full value on both, with no factor (first spec 4). | clear |
| Death: gauge below 2 ends the play at once as FAILED, as for HARD. | clear |
| Consequence for P-ATTACK: any GOOD, BAD, POOR or empty POOR kills (−100, or −60 when shown ≤ 30, which is more than the gauge then holds). An LN released early (a BAD) kills. Only PGREAT, GREAT and mines can be survived. | clear (follows from the table) |
| Clear: all notes judged and gauge ≥ 2 gives HARD CLEAR, unless FULL COMBO applies first (max combo = notes). A P-ATTACK survivor always has max combo = notes, so a P-ATTACK clear is always a FULL COMBO. A G-ATTACK clear is a HARD CLEAR or a FULL COMBO. | clear |
| Lamp caps of the first spec 7.2 apply unchanged. | clear |
| Clearing with P-ATTACK or G-ATTACK sets its own "cleared with this gauge" mark in the song's option history (one mark per gauge: GROOVE, HARD, HAZARD, EASY, P-ATTACK, G-ATTACK), which the select screen can show. An EASY CLEAR sets only the EASY mark, and only when the EASY gauge was used. | clear |
| The select-screen indicator of "which lamp this gauge is playing for" ranks P-ATTACK with HAZARD (full-combo level) and G-ATTACK with HARD. | inferred |

### 1.3 How they look

| rule | confidence |
|---|---|
| The play gauge is drawn as 50 segments of 2 % each. GROOVE and EASY outside courses use two colour sets: segments up to 78 % in the "normal" set and segments from 80 % up in the "clear" set, which is the visible 80 % border. Every other case (HARD, HAZARD, P-ATTACK, G-ATTACK, and every gauge in any course) uses the "clear" set for all 50 segments, with no border. The colours themselves come from the skin. | clear |
| So P-ATTACK and G-ATTACK look exactly like HARD: no own colour, no border line. The skin's "survival-type gauge" condition is true for them. | clear |
| Survival-type gauges start full, so the skin's "gauge full" effect runs from the start of play for HARD, HAZARD, P-ATTACK and G-ATTACK. | clear |
| On the result screen's gauge graph, the 1P graph uses the survival colouring for HARD, HAZARD, P-ATTACK, G-ATTACK and for all course plays, and the two-colour (below 80 / from 80) colouring otherwise. The 2P graph treats only HARD and HAZARD as survival, so a 2P P-ATTACK or G-ATTACK graph is coloured like GROOVE. | clear for 1P; ambiguous for 2P (looks like an oversight) |

### 1.4 In courses

| rule | confidence |
|---|---|
| Expert and NONSTOP courses: both gauges allowed, values as 1.2, start 100, ×0.6 rule, death below 2. | clear |
| Class courses: P-ATTACK is allowed with the 1.2 values. G-ATTACK is not: it is switched to GROOVE when a class course starts (5.1). | clear |
| Course lamp with either gauge: HARD CLEAR (or FULL COMBO), see 4.4. | clear |

---

## 2. Course kinds and course data

### 2.1 Where courses live; no course file

| rule | confidence |
|---|---|
| This version reads **no course file** (no `.lr2crs` or similar). Courses are rows in the song database (`LR2files\Database\song.db`), in three tables named by kind: `expert` (free / expert courses), `nonstop` (NONSTOP, connected) and `grade` (class / dan). The `main` branch is the same. | clear |
| An older single table `course` is dropped at start-up; its contents are not migrated. | clear |
| Courses get into the tables in three ways: built in the game ("NEW COURSE" entry), generated ("RANDOM MIX" entry, 2.5), or present with the `ir` flag set (internet-ranking courses, read-only). No code reading courses from difficulty tables, web pages or files was found; difficulty tables are not handled at all. | clear for what exists; that IR courses arrive from outside is inferred (no download code for them was found) |
| Course lists appear in the song select as custom folders. Three folder files are always added to the folder list: `LR2files\CustomFolder\course1.lr2folder`, `course2.lr2folder`, `course3.lr2folder`. A `.lr2folder` whose `#COMMAND` contains `__EXPERT__`, `__NONSTOP__` or `__GRADE__` lists that table instead of songs. | clear |
| Inside such a folder: all courses of that table, newest (highest id) first, then a "NEW COURSE" entry, then (not in the class folder) a "RANDOM MIX" entry. The select screen's key-mode filter applies to the course's `line` field. | clear |

### 2.2 Row fields

| field | meaning | confidence |
|---|---|---|
| `id` | integer key; list order (descending); can be swapped with another course to reorder | clear |
| `title` | course name shown in the list | clear |
| `line` | key mode of the course, taken from its first stage when created: 7, 5, 14, 10 or 9. Used only by the key-mode filter (7+5 / 7 / 5 / 14+10 / 14 / 10 / 9) | clear |
| `hash` | the course identity and content, a packed string (2.3) | clear |
| `ir` | 0 or 1. 1 = internet-ranking course: read-only in the game (joins and the flag cannot be edited) and its result is sent to the ranking; with 0 the result screen says the course is not ranking-enabled | clear |

### 2.3 The packed course string (`hash`)

The string is a 32-character header followed by one 32-character lower-case hexadecimal MD5 per stage, in stage
order (the stage chart's file hash, the same hash that identifies the song's own score).

| header characters | content | confidence |
|---|---|---|
| 1–10 | one digit per join, '0'–'5' (join after stage 1, after stage 2, …; 3.1). Meaningful only for NONSTOP; '0' in other kinds. A newly built NONSTOP course gets the player's default join setting in characters 1–5 and '0' in 6–10 | clear |
| 11 | course kind digit: '0' expert, '1' NONSTOP, '2' class | clear |
| 12–14 | '0' (unused) | clear |
| 15–28 | fourteen '0' (unused) | clear |
| 29–32 | the first four characters of the lower-case hex MD5 of the player's password | clear |

| rule | confidence |
|---|---|
| Number of stages = (length of the string ÷ 32) − 1. A course with no stage is listed but cannot be played. | clear |
| The kind used **for play** is header character 11, not the table the row sits in (the table only decides which folder lists it). They normally agree. | clear |
| A course is playable only if every stage hash is found in the song database; a missing song shows a "not owned" stage line and the course is locked. | clear |
| At most 5 stages are supported in play (loading stops at 5 for NONSTOP; per-stage paths hold 5). The list accepts up to 10 hashes, but a course of more than 5 stages behaves unpredictably. Treat 5 as the maximum. | clear for 5 in NONSTOP; ambiguous above 5 for expert/class |
| The course's **identity** is the whole packed string. Its best score, lamp and counts are stored in the player's score database under that string, in the same table as song scores. Because characters 29–32 depend on the password and 1–10 on the joins, the same song list built by two players, or the same NONSTOP course with an edited join, is a different course with separate records. A NONSTOP replay file is named by the MD5 of the packed string. | clear |
| Editing a course rebuilds the header from the course's current joins and kind and the current player's password, then appends the stage hashes, so an edit can change its identity. Read-only (`ir` = 1) courses are never rewritten. | clear |

### 2.4 What a course can and cannot carry

| rule | confidence |
|---|---|
| A course carries only: its songs (in order), its kind, and (NONSTOP) one join type per join. It carries **no** gauge, hi-speed, random, judge or other option constraint. | clear |
| The settings file has a "default course gauge" value and the option-string file has entries for course gauge, course type and "option ban", but none of them changes play in this version: the default gauge is stored and displayed, never applied. | clear |
| The restrictions of class courses are fixed rules applied to every class course (5.1), not data. | clear |
| Hi-speed is never changed by any course kind. | clear |

### 2.5 RANDOM MIX (expert and NONSTOP only)

| rule | confidence |
|---|---|
| Choosing RANDOM MIX replaces the previous course titled "RANDOM MIX" in that table with a new one. | clear |
| Settings, from the `<course>` section of the player's config: `stage` (stage count; if not 1–5, a random count 2–5), `minlevel`/`maxlevel` (level limits, 0 = none; swapped if reversed), `minbpm`/`maxbpm` (BPM limits, 0 = none), `bpmrange` (default 10), `optimumlevel_7` (preferred level), `defaultconnection` (join type for NONSTOP). | clear |
| Songs are taken from the song database for the key mode currently filtered, in random order, within the limits. With `bpmrange` > 0 the first song must have a single BPM, and later songs must lie within ± `bpmrange` of the first song's BPM. No two stages come from the same song folder. If `optimumlevel_7` ≥ 1, each picked song is replaced by the chart in its folder whose level is closest to that value (within the limits); the 7-key value is used whatever the key mode. | clear; the 7-key value for all modes is inferred to be an oversight |

---

## 3. NONSTOP courses

A NONSTOP course is loaded as **one chart** built from all stages, and played as one play.

### 3.1 Join types

| digit | name used here | tempo matched | stage N sound after the join | stage N+1 sound before the join | placement of stage N+1 |
|---|---|---|---|---|---|
| 0 | fade | no | fades out over 10 s | fades in over the 5 s before the join | trimmed lead-in, tight (3.2) |
| 1 | fade + tempo match | yes | not faded | fades in over the 5 s before the join | trimmed lead-in, tight |
| 2 | cut | no | fades out over 200 ms | fades in over the 200 ms before the join | trimmed lead-in, tight |
| 3 | cut + tempo match | yes | fades out over 200 ms | fades in over the 200 ms before the join | trimmed lead-in, tight |
| 4 | gap 1 | no | not faded | fades in over the 5 s before the join | trimmed lead-in, loose (3.2) |
| 5 | gap 2 | no | not faded | not faded | no trimming; join moved ≥ 5 s later (3.2) |

(**clear** for the numbers. The menu text for each join type comes from the theme's option-string file
`#COURSE_TSUNAGI` line, six entries; the names above are descriptive only.)

The fades apply to both that stage's background sound and its keysounds; each stage's sounds play on their own
channel group with their own volume and pitch. A stage with no fade-in plays at full volume from the start. The
volume ramps are linear in a value that is then squared before it is applied (so the loudness curve is quadratic).
(**clear**)

BGA: at every join, whatever its type, stage N's BGA fades out over the 3 s after the join and stage N+1's BGA fades
in over the 3 s before it (the two cross-fade). Each stage keeps its own BGA layers. (**clear**)

### 3.2 Where stage N+1 is placed

All positions below are on stage N's or stage N+1's own chart; times follow from each chart's own BPM and measure
lengths.

1. **The join bar.** Find stage N's last note: the latest object on a visible note channel or a long-note channel
   (mines and background objects are not counted). The join bar is the bar line of stage N nearest to that note;
   on a tie, the earlier bar line. It can therefore be up to half a measure **before** stage N's last note.
   (**clear**)
2. **Gap 2 only:** the join bar moves to the first bar line of stage N whose time is more than 5000 ms after the bar
   found in step 1 (stage N's tempo and measure lengths continue past its end for this purpose). (**clear**)
3. **The aligned bar of stage N+1.** Let p be the position of stage N+1's first note, in measures from the start of
   its chart. Choose a whole measure number m:
   - fade, fade + tempo match, cut, cut + tempo match: m is the smallest whole number with m ≥ p − 0.5 (the nearest
     bar line to the first note, a half rounding down);
   - gap 1: m is the smallest whole number with m ≥ p − 1.5 (the first note lands 0.5 to 1.5 measures after the join);
   - gap 2: m = 0 (no trimming).
   (**clear** for the second stage. The "first note" is in effect the first visible note; the reading also lets an
   object of any channel that is written later in the file but sits earlier in time lower p, which normally changes
   nothing: **ambiguous**.)
4. Stage N+1's whole chart is shifted in time so that the start of its measure m falls exactly on the join bar's time.
   Its measures before m (its lead-in) therefore play **before** the join, overlapping stage N's end; their
   background sounds are heard (fading in as in 3.1) but their bar lines are not drawn. (**clear**)
5. From the join bar on, stage N's remaining bar lines and BPM changes are discarded; the scroll follows stage N+1.
   Stage N's notes, background sounds and BGA after the join bar keep their own times. (**clear**)
6. **Overlap clean-up.** Let L be the time of stage N's last note, long-note head or mine. Every note (or LN head or
   mine) of stage N+1 at or before L + 100 ms is turned into a background sound (its keysound plays by itself; it is
   not a note, is not counted in the note count and cannot be judged), with one exception: a note within ±100 ms of
   L (strictly less than 100 ms away) stays playable if stage N has no object in the same lane in the last 100 ms
   before L. (**clear**)

Consequences:

| rule | confidence |
|---|---|
| There is **no fixed time gap** between stages. With the tight joins, stage N+1's first note lands within about half a measure (its measures) of the bar nearest stage N's last note, so the gap is roughly −½ to +1 measure, and N+1 notes that would come too early are silently turned into background sound. | clear |
| Stage N+1 is not offset by stage N's last object or by its song length; only stage N's last **note** and the bar grid matter. Stage N's trailing background sound simply continues past the join (faded or not, 3.1). | clear |
| Third and later stages: the reading compares the first note with stage N's join bar after subtracting stage N's own trim (its m), so from the third stage on, stage N+1 is placed that many measures **earlier** than the rule above gives (more of its opening becomes background sound). | ambiguous (self-inconsistent; looks like an artefact; implement the second-stage rule for every stage) |
| Gap 1 with a first note in the first half of measure 0 gives m = −1, which the reading cannot handle. | ambiguous (treat as m = 0) |

### 3.3 Tempo, scroll and hi-speed

| rule | confidence |
|---|---|
| Tempo match (join types 1 and 3): when stage N+1's `#BPM` header is read, the whole of stage N+1 is played faster or slower by the ratio r = (stage N's final BPM as played) ÷ (stage N+1's `#BPM` header value). All its BPMs, stops and times scale by r, and its sound is played at pitch × r (speed and pitch change together, like a turntable). The ratio is worked out afresh at each join from the original speed, so it does not compound. | clear |
| Tempo match uses only the header `#BPM`; a stage with no `#BPM` header line is not matched. `#BPMxx` definitions written before the `#BPM` header line are not scaled. | clear for the first; inferred (order effect) for the second |
| Judge windows are not scaled by r. | clear |
| Scroll in a NONSTOP course is **time-based**: a note's distance from the judge line depends only on time, as if every stage were a constant 150 BPM chart at the player's hi-speed. BPM changes do not change the scroll speed; a STOP still freezes the scroll. The hi-speed-fix option is ignored. This forced constant scroll does **not** cap the lamp (the constant-hi-speed lamp cap of the first spec 7.2 applies only when the player chose that option). | clear |
| The player's hi-speed is kept across all stages (it is one play). | clear |
| `#BPMxx` and `#STOPxx` definitions are shared by all stages: a later stage's definition replaces an earlier one with the same number, and a number a later stage does not define keeps the earlier stage's value. | clear (edge case) |

### 3.4 Keysounds and BGA identity: the stage value offset

| rule | confidence |
|---|---|
| To keep stages apart, every object value of the k-th stage (k counted from 0, so the second stage has k = 1) is raised by 1296·k: keysound numbers, BGA image numbers, the `#LNOBJ` value and **mine values**. BPM-change and STOP references are not raised. So each stage uses its own `#WAVxx` and `#BMPxx` files. | clear (same on `main`, with its own slot size) |
| The mine artefact (correcting the first spec 4): a mine's damage is its value, so a mine in the second stage costs at least 1296, in the third at least 2592, and so on. On HARD, HAZARD, P-ATTACK and G-ATTACK that is an instant fail. On GROOVE and EASY the mine floor still applies, so the gauge drops to exactly 2 % and the play continues (the next BAD, POOR or empty POOR then kills, since a course has no 2 % floor for judgements). First-stage mines are normal. | clear in the reading; that this is unintended is inferred |

### 3.5 Judge windows

| rule | confidence |
|---|---|
| The first stage's `#RANK` sets the windows with the normal table (first spec 2.1), including the two maniac judge options as in the first spec 2.3. | clear |
| A later stage's `#RANK` switches the windows with the NONSTOP table: 0 → 12/24/60/200, 1 → 15/30/80/200, 3 → 21/60/120/200, any other value → 18/40/100/200 (PGREAT/GREAT/GOOD/BAD, ms), empty POOR up to 1000 ms as always. The value's whole part is used (`2.5` → 2; no leading digits → 0). Same on `main`. | clear |
| Under the two maniac judge options, a later stage's `#RANK` gives 12/24/60/200 and 12/12/12/200 (not 8/24/40 and 8/12/12 as on the first stage). | clear in the reading; ambiguous whether intended |
| A later stage without `#RANK` sends no switch: the windows of the previous stage stay. | clear |
| When: the switch happens at the time of stage N+1's own chart start (its measure 0) after the shift of 3.2, which is the join time minus the length of N+1's trimmed lead-in (for gap 2: exactly at the join). Stage N's last notes can therefore be judged with stage N+1's windows. | inferred |
| The switch changes only the 1P side's windows. | clear |
| Everything that depends on the windows follows the switch: LN release limit (GOOD), LN head miss (GOOD late), mine press trigger (GREAT). | inferred |

### 3.6 TOTAL, HARD factor and gauge

| rule | confidence |
|---|---|
| n = all playable notes of all stages together (after the overlap clean-up of 3.2). | clear |
| TOTAL = the value of the last `#TOTAL` line read, going through the stages in order: a stage without `#TOTAL` leaves the previous value in force. If that final value is below 1 (or no stage has `#TOTAL`), the default of the first spec 3.3 is computed from the combined n. | clear (confirms the first spec's inference) |
| f (HARD factor) is computed once from that TOTAL and the combined n. It matters only for HARD and G-ATTACK, since NONSTOP's GROOVE and EASY values are constants. | clear |
| Gauge: one gauge for the whole play, start 100, the expert/NONSTOP values of the first spec 3.6 (P-ATTACK, G-ATTACK and HAZARD as in single play), ×0.6 rule on every gauge, no 2 % floor for judgements. It simply continues across joins. | clear |
| Failure: the gauge below 2 at any moment ends the play at once as FAILED; later stages are not played. | clear |

### 3.7 Combo, `#RANDOM`, current stage

| rule | confidence |
|---|---|
| Combo runs on across joins (one play). Max combo and FULL COMBO are over the whole course. | clear |
| `#RANDOM` is rolled per stage as each stage's `#RANDOM` line is read, from one random sequence seeded once for the whole play (a retry keeps the seed). The open-`#IF` state is not reset between stages: a stage that ends inside an unclosed, untaken `#IF` hides the next stage's lines until its first `#RANDOM` or `#ENDIF`; an `#IF` before any `#RANDOM` in a later stage compares with the previous stage's roll. | clear for per-stage rolls; inferred for the carry-over edge cases |
| The skin's "current stage" and the shown stage title advance only at gap-1 and gap-2 joins; with the other join types they stay on the first stage for the whole play. | ambiguous (same on `main`; looks like an artefact) |

### 3.8 Result and saving

| rule | confidence |
|---|---|
| After the play, the normal result screen (not the course result screen) shows one result for the whole course: counts, EX score, score, max combo, BP and lamp over all stages together. | clear |
| Lamp: FULL COMBO if max combo = n; otherwise, all notes judged and gauge ≥ 2 → HARD CLEAR (HARD, P-ATTACK, G-ATTACK), EASY CLEAR (EASY), CLEAR (GROOVE, HAZARD); otherwise FAILED. No 80 % rule. Then the normal lamp caps (first spec 7.2) apply. | clear |
| The result is saved like a single song, under the course's identity (2.3): best EX (strictly higher replaces), lamp (only raised), max combo, min BP, play/clear/fail counts, replay by the player's replay setting. The stage songs' own records are **not** touched. | clear |
| Retry from the result screen is allowed (as for a single song). | clear |
| The audio pitch option is switched off when a NONSTOP course starts. | clear |

---

## 4. Expert (free) courses

Each stage is loaded and played as its own play; between stages the stage result screen is shown, with no song
select or decide screen.

### 4.1 Gauge

| rule | confidence |
|---|---|
| Values: the expert/NONSTOP row of the first spec 3.6 (GROOVE +0.1/+0.1/+0.04, −1.5/−2/−1.5; EASY +0.12/+0.12/+0.048, −1.2/−1.6/−1.2; HARD +0.1/+0.1/+0.05, −6f/−10f/−2f; HAZARD, P-ATTACK, G-ATTACK as single play). Every gauge is allowed. | clear |
| f is per stage, from that stage's own TOTAL and note count. | clear |
| Start 100 at the first stage only; afterwards the gauge carries over with its exact value (not the shown value). ×0.6 rule on every gauge; no 2 % floor for judgements; mines still stop at 2 % on GROOVE/EASY. | clear |

### 4.2 What carries over and what does not

| carried to the next stage | not carried (per stage) |
|---|---|
| gauge value | judge windows (each stage's own `#RANK`, normal table) |
| course combo and course max combo (the play screen shows the course combo during course play) | TOTAL, f, note count of the stage |
| course totals of PGREAT, GREAT, GOOD, BAD, POOR (empty POORs included, mines **not** included) | the stage's own combo and max combo (used for that stage's own result) |
| count of judged notes, running total of notes | BGA, keysounds, BPM, scroll |
| the random seed (every stage is shuffled and `#RANDOM`-rolled from the same seed) | |
| the player's options (nothing is forced) | |

(**clear**, except the random-seed reuse, which is **inferred**: the seed is set once per course and never reset
between stages.)

Mines add to the stage's own POOR count but not to the course totals, so the course result's POOR and BP leave mines
out. (**clear** in the reading; may be an artefact.)

### 4.3 Stage end and failure

| rule | confidence |
|---|---|
| A stage's own lamp is decided with the course rule: FULL COMBO first; HARD/P-ATTACK/G-ATTACK need all notes judged and gauge ≥ 2; GROOVE/EASY/HAZARD also need only all notes judged and gauge ≥ 2 (no 80 %). | clear |
| If the gauge falls below 2 during a stage, the play ends at once; after that stage's result screen the course result follows and the course is FAILED. Quitting a stage (gauge set to 0) does the same. | clear |
| If a stage ends with gauge ≥ 2 and it is not the last stage, the next stage starts directly. | clear |
| No retry in expert or class courses. | clear |

### 4.4 Course result, lamp and saving

| rule | confidence |
|---|---|
| Order of screens: stage 1 play, stage 1 result, stage 2 play, stage 2 result, …, last stage result, course result. On failure: the failed stage's result, then the course result. | clear |
| Course result screen (its own skin) shows the course totals: judgement counts summed over the stages played, EX score from them, course max combo, total notes. | clear |
| Course lamp: FAILED if the final gauge is below 2 or the last stage was not reached; else FULL COMBO if course max combo = course total notes; else, all notes judged and gauge **strictly above** 2 → HARD CLEAR (HARD, P-ATTACK, G-ATTACK), EASY CLEAR (EASY), CLEAR (GROOVE, HAZARD); otherwise FAILED. (A course that ends at exactly 2.0 is FAILED, although each stage would have cleared at 2.0.) | clear |
| No lamp cap is applied to the course lamp for assist options, H-RANDOM-type options or constant hi-speed; only the "no save" conditions (autoplay, replay, the speed and other no-save options) stop the course record from being saved. | clear in the reading; possibly an oversight |
| Saved under the course identity: lamp (only raised), best EX with its counts (strictly higher replaces), course max combo, min BP, play count, clear count (lamp ≥ EASY CLEAR) or fail count. Min BP = BAD + POOR, plus the unplayed notes when the course was not finished. | clear |
| Total notes on a course that ends early should be the sum of all stages' note counts; this branch adds the remaining stages counted from the stage **before** the failed one (double-counting two stages), using each song's stored note count (0 for a song never played). `main` adds only the stages not played. | ambiguous; implement `main`'s rule (sum of all stages) |
| The 200 000-point score saved for the course is not recomputed from the course totals (it is the last stage's). | ambiguous (recompute it from the totals) |
| Each stage's result is also written into that **song's own** record, but only partly: best EX and counts (strictly higher), max combo, min BP and option history are updated; the clear lamp is updated **only to FULL COMBO** (a stage clear does not raise the song's lamp otherwise); the song's play, clear and fail counts are not changed. This happens only when no assist, battle, extra, H-RANDOM-type or constant hi-speed option is on. | clear |
| Replays are saved per stage. | clear |
| The course result plays the "course clear" sound for any lamp above FAILED, else the "course failed" sound. | clear |

---

## 5. Class (dan) courses

Everything in section 4 applies, with these differences.

### 5.1 Forced settings at class course start

| rule | confidence |
|---|---|
| Gauge: EASY is switched to GROOVE and G-ATTACK is switched to GROOVE (both sides). GROOVE, HARD, HAZARD and P-ATTACK are kept. | clear |
| Random is set to OFF on both sides. Hidden/sudden, assist options, battle, the extra mode, DP flip, the audio pitch option and all the "maniac" note and display modifiers are switched off. | clear |
| These are changes to the player's own settings and are not restored afterwards. | inferred |
| Hi-speed and judge timing are left as they are. | clear |

### 5.2 Gauge

| rule | confidence |
|---|---|
| GROOVE: +0.1, +0.1, +0.04, −2, −3, −2 (first spec 3.6). HARD: +0.1, +0.1, +0.04, −6f, −10f, −2f. HAZARD and P-ATTACK as single play. | clear |
| No EASY or G-ATTACK class values are ever used (5.1). | clear |

### 5.3 Pass, lamp and records

| rule | confidence |
|---|---|
| A class course is passed when every stage is played to its end with the gauge never below 2 and the final gauge strictly above 2: course lamp CLEAR (GROOVE or HAZARD), HARD CLEAR (HARD or P-ATTACK), or FULL COMBO. There is no other pass condition (no score or BP limit). | clear |
| The saved record is the course lamp and course counts under the course identity, as in 4.4. The player profile's per-key-mode dan fields are loaded and saved but never set by a course result, so no dan title is recorded beyond the course's own lamp. | clear in the reading |
| No RANDOM MIX is offered for class courses. | clear |
| Class courses are listed only in the folder whose command contains `__GRADE__`. | clear |

---

## Changes to LR2_BEHAVIOUR.md

| section | first spec says | correction or addition |
|---|---|---|
| 3.2 | P-ATTACK and G-ATTACK rows, "clear" | Confirmed, including G-ATTACK's PGREAT −10·f (identical in all three gauge tables and on `main`). Add: P-ATTACK dies on any GOOD or worse and on empty POOR, so a P-ATTACK clear is always FULL COMBO. Add the option order GROOVE, HARD, HAZARD, EASY, P-ATTACK, G-ATTACK (1.1). |
| 3.1 | clear threshold: GROOVE, EASY, HAZARD need HP ≥ 80 | Only outside courses. In any course (a stage's own lamp, a NONSTOP play) they need only HP ≥ 2. |
| 3.6 table | class row "GROOVE or EASY"; "any: HAZARD, P-ATTACK, G-ATTACK as single play" | EASY and G-ATTACK are switched to GROOVE at the start of a class course, so the class row applies to GROOVE only, and class has no G-ATTACK. Random and many options are also forced off (5.1). |
| 3.6 | NONSTOP f "from all stages' notes together and the last `#TOTAL` read: inferred" | Clear. Precisely: the last `#TOTAL` line in stage order (a stage without one keeps the previous value); default TOTAL from the combined note count if that value is below 1. |
| 3.6 | course clear "each needing HP strictly above 2" | True for the expert/class course lamp only. Each stage's own lamp and the NONSTOP lamp use HP ≥ 2. |
| 3.6 / 7.2 | lamp caps | The expert/class course lamp gets no lamp caps (4.4); NONSTOP gets the normal caps. NONSTOP's forced time-based scroll does not trigger the constant-hi-speed cap. |
| 2.2 | NONSTOP `#RANK` table | Confirmed. Add: it changes only the 1P side; a later stage without `#RANK` keeps the previous windows; under the maniac judge options a later stage gives 12/24/60/200 and 12/12/12/200; the switch happens at stage N+1's shifted chart start (3.5). |
| 4 | NONSTOP mine row: "1296 × (stage index) … (an instant fail)" | The stage index counts from 0 (second stage +1296). Instant fail only on HARD, HAZARD, P-ATTACK and G-ATTACK; on GROOVE and EASY the gauge drops to exactly 2 % and play continues. Only NONSTOP is affected (expert and class load each stage alone). |
| 6 | POOR count includes exploded mines | In expert and class course **totals**, mines are not counted (each stage's own counts do include them). |
| new | — | Course storage, packed course string and identity (2); NONSTOP join rules, fades, tempo match, time-based scroll (3); per-stage saving into song records with FULL COMBO as the only lamp written (4.4). |

## Not determined

- The menu texts of the gauge values and of the six join types (they come from the theme's option-string file).
- Whether the third-and-later NONSTOP stage placement (3.2) and the "current stage" display (3.7) behave as read in the
  original binary.
- How LR2 obtained `ir` = 1 courses (no download code for them was found in either branch).
- Behaviour of expert/class courses with more than five stages.
