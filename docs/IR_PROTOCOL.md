# vibeBMS JSON IR protocol (version 2)

A minimal REST protocol so that any IR server can accept vibeBMS scores. Configure it in
`SETTINGS › ONLINE` (base URL, API token). All bodies are UTF-8 JSON. The client is
`crates/vibebms-ir/src/providers/vibe_json.rs`.

Authentication: `Authorization: Bearer <token>` on every request when a token is set. Servers
hand out tokens on their web site; the player pastes it in settings.

Version 2 adds fields and routes; a version 1 server keeps working: it ignores the new body
fields, and the client treats a 404 on an optional route as "not supported".

## `GET {base}/me` (version 2, optional)
Checks the token. Response:
```json
{"id": "123", "name": "player name", "can_submit": true,
 "chart_page": "https://ir.example/charts/{sha256}"}
```
401 or 403: the token is refused. 404: a version 1 server; the token is used as it is.
`chart_page` (optional): the server's web page of a chart, an `http(s)` address with `{sha256}` and/or `{md5}`
in it; song select's IR PAGE (key I) opens it in the browser. Without it the server has no chart pages here.

## `GET {base}/ranking?sha256=...&md5=...&lntype=LN|CN|HCN[&limit=N]`
Version 1 response: an array of rows. Version 2 may answer an object instead, with the number
of players who have a score on the chart:
```json
{"players": 345, "entries": [
  {"rank": 1, "player": "name", "player_id": "123", "clear": "Hard", "ex_score": 1800,
   "pgreat": 800, "great": 200, "max_combo": 999, "min_bp": 3, "notes": 1000}]}
```
`clear` is one of `NoPlay, Failed, AssistEasy, LightAssistEasy, Easy, Normal, Hard, ExHard, FullCombo, Perfect, Max`.
`limit` (version 2) is how many rows the client wants; without it, the server chooses.
A server may send an `ETag`; the client then sends `If-None-Match` and takes `304` as "unchanged".

## `POST {base}/scores`
Body (`ScoreSubmission` in `crates/vibebms-ir`), field names follow beatoraja's `IRScoreData`:
```json
{"sha256": "...", "md5": "...", "lntype": "LN", "clear": "Hard",
 "epg": 400, "lpg": 380, "egr": 50, "lgr": 60, "egd": 3, "lgd": 2, "ebd": 1, "lbd": 0,
 "epr": 0, "lpr": 2, "ems": 0, "lms": 1, "maxcombo": 500, "notes": 900, "passnotes": 900,
 "minbp": 4, "exscore": 1690, "option": "RANDOM", "seed": 123456, "gauge": "HARD",
 "judge_preset": "beatoraja / beatoraja", "device": "KEYBOARD/CONTROLLER",
 "date": 1767225600, "client": "vibeBMS 0.1.0",
 "v": 2, "play_id": "9f2c0a…", "rule": "vibe-beatoraja", "date_ms": 1767225600123,
 "devices": {"keyboard": 912, "gamepad": 0, "touch": 0, "touch_ui": false, "platform": "Windows"}}
```
- `e*` = early (FAST), `l*` = late (SLOW); `pr` = POOR (a note missed), `ms` = empty POOR.
  A judgement with no timing error at all counts as early.
- `gauge` is the gauge the play ended on; `option` the lane options per side.
- Version 2 fields:
  - `v`: 2.
  - `play_id`: the play's id. The same play is always sent with the same id and the same body.
  - `rule`: the rule profile the play was judged under: `vibe-lr2` or `vibe-beatoraja`. Only
    plays under one of them are sent.
  - `date_ms`: `date` in milliseconds. Both are when the play ended, taken once.
  - `devices`: the presses that reached the judge per device, whether the touch controls were
    on screen, and the platform (`Windows`, `MacOs`, `Linux`, `Web`, `Android`, `Ios`, `Other`).
    Touch and web plays are sent too, labelled: the server decides how to rank them.

Headers (version 2): `Idempotency-Key: <key>`, the same key on every attempt to send one play
to one account. A server that already has the key, or the `play_id`, should answer 2xx or 409
without storing the play again.

Answers:
- 2xx: stored. 409: already stored. Both end the submission.
- 401 / 403: the token is refused. The client stops sending to this server until the user
  checks the account again.
- 408, 429, 5xx: tried again later, with a growing delay; a `Retry-After` (seconds or an HTTP
  date) is the shortest wait.
- Any other status: refused for good. A JSON body `{"error": "why"}` is shown to the user.

## `GET {base}/players/{id}/lamps` (version 2, optional)
One player's lamps, for rival lamps in song select:
```json
{"player_name": "name", "lamps": [
  {"sha256": "...", "md5": "...", "clear": "Hard", "ex_score": 1800, "min_bp": 3,
   "date_ms": 1767225600123}]}
```
`ex_score`, `min_bp` and `date_ms` may be left out. `ETag` and `304` as for the ranking.

## `GET {base}/rivals` (version 2, optional)
The rivals the user set on the server: `[{"id": "55", "name": "rival"}]`.

## Browsers (CORS)
The web build calls the server from the page, so the server must answer CORS:
- `Access-Control-Allow-Origin` with the page's origin (or `*`).
- On preflight (`OPTIONS`): `Access-Control-Allow-Headers: Authorization, Content-Type,
  Idempotency-Key, If-None-Match` and `Access-Control-Allow-Methods: GET, POST`.
- `Access-Control-Expose-Headers: Retry-After, ETag`. A page cannot read these two headers
  otherwise: without them the client falls back to its own delays and never sees a `304`.
- CORS headers on error answers too, or the page sees a failed connection instead of the
  status and the error.

The web build never sends IR requests through a relay: a server without CORS is not available
in the browser.

## Other IRs
A service with its own API can be supported by putting a small server speaking this protocol in
front of it, or by an adapter of its own in `crates/vibebms-ir/src/providers/`.
