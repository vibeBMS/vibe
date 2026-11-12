# vibeBMS JSON IR protocol

A minimal REST protocol so that any IR server can accept vibeBMS scores. Configure it in
`SETTINGS › IR` (kind `JSON`, base URL, API token). All bodies are UTF-8 JSON.

Authentication: `Authorization: Bearer <token>` on every request once a token is known.

## `POST {base}/login`
Request `{"user": "...", "password": "..."}` → `{"token": "..."}`.
(Servers may instead hand out tokens on their web site; the token can be pasted in settings.)

## `GET {base}/ranking?sha256=...&md5=...&lntype=LN|CN|HCN`
Response: array of
```json
{"rank": 1, "player": "name", "player_id": "123", "clear": "Hard", "ex_score": 1800,
 "pgreat": 800, "great": 200, "max_combo": 999, "min_bp": 3, "notes": 1000}
```
`clear` is one of `NoPlay, Failed, AssistEasy, LightAssistEasy, Easy, Normal, Hard, ExHard, FullCombo, Perfect, Max`.

## `POST {base}/scores`
Body (`ScoreSubmission` in `crates/vibebms-ir`), field names follow beatoraja's `IRScoreData`:
```json
{"sha256": "...", "md5": "...", "lntype": "LN", "clear": "Hard",
 "epg": 400, "lpg": 380, "egr": 50, "lgr": 60, "egd": 3, "lgd": 2, "ebd": 1, "lbd": 0,
 "epr": 0, "lpr": 2, "ems": 0, "lms": 1, "maxcombo": 500, "notes": 900, "passnotes": 900,
 "minbp": 4, "exscore": 1690, "option": "RANDOM", "seed": 123456, "gauge": "HARD",
 "judge_preset": "beatoraja / beatoraja", "device": "KEYBOARD/CONTROLLER",
 "date": 1767225600, "client": "vibeBMS 0.1.0"}
```
`e*` = early (FAST), `l*` = late (SLOW); `ms` = empty POOR. Respond with any 2xx on success.

## LR2IR
The built-in LR2IR client only reads rankings from
`{base}/2/getrankingxml.cgi?id=1&songmd5=<md5>` (no submission).

## Adding another IR
Implement `vibebms_ir::IrClient` and register it in `create_client` (`crates/vibebms-ir/src/lib.rs`).
