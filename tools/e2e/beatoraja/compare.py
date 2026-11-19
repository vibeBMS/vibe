#!/usr/bin/env python3
"""Compares vibeBMS's results (the e2e_play example's JSON lines) with
beatoraja's (run.py's results) case by case.

    python3 compare.py --vibe vibe.jsonl --beatoraja beatoraja.json
                       [--expected expected.json] [--md summary.md] [--json out.json]

Compared: the counts of each judgement (PGREAT .. POOR, empty POOR), the EX
score, the max combo, FAST and SLOW (GREAT to BAD early and late), the
gauge at the end (to 0.05) and the clear lamp. A difference listed in
expected.json (a known difference: intended, with the docs/RULES.md section
that says so, a vibeBMS bug or the harness's) is reported but passes; any other fails (exit 1), as does a
case beatoraja gave no result for. A case beatoraja answered differently in
two plays at normal speed (run.py --recheck) is reported as unstable and
does not fail.
"""

import argparse
import fnmatch
import json
import sys

HERE = __file__.rsplit("/", 1)[0]
JUDGES = ["pg", "gr", "gd", "bd", "pr", "ms"]
LAMPS = ["NO PLAY", "FAILED", "ASSIST", "L-ASSIST", "EASY", "CLEAR", "HARD", "EX-HARD",
         "FULL COMBO", "PERFECT", "MAX"]
GAUGE_TOLERANCE = 0.05


def vibe_values(v):
    j = v["judges"]
    out = {k: j[k]["early"] + j[k]["late"] + j[k]["just"] for k in JUDGES}
    out.update({
        "ex": v["ex"], "max_combo": v["max_combo"], "fast": v["fast"], "slow": v["slow"],
        "gauge": round(v["gauge"], 2), "clear": LAMPS[v["clear"]], "notes": v["notes"],
    })
    return out


def bja_values(b):
    s = b["score"]
    out = {k: s["E" + k] + s["L" + k] for k in JUDGES}
    out.update({
        "ex": s["Exscore"], "max_combo": s["Combo"],
        "fast": s["Egr"] + s["Egd"] + s["Ebd"], "slow": s["Lgr"] + s["Lgd"] + s["Lbd"],
        "gauge": round(b["gauge"], 2), "clear": LAMPS[s["Clear"]], "notes": s["Notes"],
    })
    return out


def differs(field, a, b):
    if field == "gauge":
        return abs(a - b) > GAUGE_TOLERANCE
    return a != b


def read_vibe(path):
    vibe = {}
    for line in open(path):
        if line.strip():
            v = json.loads(line)
            vibe[v.get("name") or v.get("case")] = v
    return vibe


def field_diffs(v, b):
    """The fields where vibeBMS's result v and beatoraja's b differ."""
    vv, bv = vibe_values(v), bja_values(b)
    return [f for f in vv if differs(f, vv[f], bv[f])]


def expected_for(expected, name, field):
    for e in expected:
        if any(fnmatch.fnmatch(name, p) for p in e["cases"]) and (
                "*" in e["fields"] or field in e["fields"]):
            return e
    return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--vibe", required=True)
    ap.add_argument("--beatoraja", required=True)
    ap.add_argument("--expected", default=f"{HERE}/expected.json")
    ap.add_argument("--md")
    ap.add_argument("--json")
    a = ap.parse_args()
    vibe = read_vibe(a.vibe)
    bja = {b["name"]: b for b in json.load(open(a.beatoraja))}
    expected = json.load(open(a.expected))["differences"]
    rows = []
    for name in sorted(set(vibe) | set(bja)):
        v, b = vibe.get(name), bja.get(name)
        row = {"name": name, "diffs": [], "status": "match"}
        if v is None or "error" in v:
            row.update(status="error", error="vibeBMS: " + (v or {}).get("error", "no result"))
        elif b is None or "error" in b:
            row.update(status="error", error="beatoraja: " + (b or {}).get("error", "no result"))
        else:
            vv, bv = vibe_values(v), bja_values(b)
            row["vibe"], row["beatoraja"] = vv, bv
            for f in vv:
                if differs(f, vv[f], bv[f]):
                    e = expected_for(expected, name, f)
                    row["diffs"].append({
                        "field": f, "vibe": vv[f], "beatoraja": bv[f],
                        "expected": e is not None,
                        "reason": e["reason"] if e else None,
                        "kind": e.get("kind", "intended") if e else None,
                        "rules": e.get("rules") if e else None,
                    })
            if b.get("unstable") and any(not d["expected"] for d in row["diffs"]):
                # beatoraja answered differently twice at normal speed: a key
                # read late (the machine was busy), not a rule
                row["status"] = "unstable"
            elif any(not d["expected"] for d in row["diffs"]):
                row["status"] = "mismatch"
            elif row["diffs"]:
                row["status"] = "expected"
        rows.append(row)
    counts = {s: sum(r["status"] == s for r in rows)
              for s in ("match", "expected", "mismatch", "unstable", "error")}
    md = ["## Behaviour: vibeBMS against beatoraja", "",
          f"{len(rows)} cases: {counts['match']} match, {counts['expected']} differ as expected, "
          f"{counts['mismatch']} differ, {counts['unstable']} unstable in beatoraja (not counted), "
          f"{counts['error']} without a result.", "",
          "| case | result | differences (vibeBMS / beatoraja) |", "|---|---|---|"]
    label = {"match": "match", "expected": "known difference", "mismatch": "**MISMATCH**",
             "unstable": "unstable (beatoraja)", "error": "**no result**"}
    for r in rows:
        if r["status"] == "error":
            detail = r["error"]
        else:
            detail = "; ".join(
                f"{d['field']} {d['vibe']} / {d['beatoraja']}" + (" (expected)" if d["expected"] else "")
                for d in r["diffs"])
        md.append(f"| {r['name']} | {label[r['status']]} | {detail} |")
    reasons = {}
    for r in rows:
        for d in r["diffs"]:
            if d["expected"]:
                reasons.setdefault(d["reason"], (d["rules"], d["kind"], []))[2].append(r["name"])
    if reasons:
        md += ["", "Known differences (expected.json):", ""]
        for reason, (rules, kind, names) in reasons.items():
            md.append(f"- **{kind}**: {reason} (docs/RULES.md, {rules}): "
                      f"{', '.join(sorted(set(names)))}")
    text = "\n".join(md) + "\n"
    if a.md:
        open(a.md, "w").write(text)
    else:
        print(text)
    if a.json:
        json.dump({"counts": counts, "cases": rows}, open(a.json, "w"), indent=1)
    sys.exit(1 if counts["mismatch"] or counts["error"] else 0)


if __name__ == "__main__":
    main()
