"""Timing of SendInput presses: lr2probe's send.csv (when each press was due
and when SendInput returned) against recv.csv (when the receiving window
saw it, by window message and by DirectInput), all in microseconds of the
shared performance counter. Prints a table; with a third argument, also
writes it as JSON.

    python jitter.py send.csv recv.csv [out.json]
"""

import csv
import json
import statistics
import sys

SCAN_Z = 0x2C


def stats(xs):
    xs = sorted(xs)
    if not xs:
        return None
    pick = lambda q: xs[min(len(xs) - 1, int(q * len(xs)))]
    return {
        "n": len(xs),
        "mean_ms": statistics.fmean(xs) / 1000,
        "sd_ms": (statistics.pstdev(xs) if len(xs) > 1 else 0) / 1000,
        "min_ms": xs[0] / 1000,
        "p50_ms": pick(0.5) / 1000,
        "p99_ms": pick(0.99) / 1000,
        "max_ms": xs[-1] / 1000,
    }


def main():
    send = list(csv.DictReader(open(sys.argv[1])))
    recv = list(csv.DictReader(open(sys.argv[2])))
    sched = [int(r["sched_us"]) for r in send]
    sent = [int(r["sent_us"]) for r in send]
    out = {"presses": len(sched), "sendinput_late": stats([b - a for a, b in zip(sched, sent)])}
    for src, name in (("m", "window_message"), ("d", "directinput")):
        downs = [int(r["t_us"]) for r in recv
                 if r["src"] == src and int(r["scan"]) == SCAN_Z and r["down"] == "1"]
        # each press is matched to the first down seen after it was due
        lat, j = [], 0
        for s in sched:
            while j < len(downs) and downs[j] < s:
                j += 1
            if j < len(downs) and downs[j] - s < 50_000:
                lat.append(downs[j] - s)
                j += 1
        out[name] = {"received": len(downs), "matched": len(lat), "late_after_due": stats(lat)}
    print(f"presses sent: {out['presses']}")
    rows = [("SendInput returned", out["sendinput_late"])] + [
        (f"{k.replace('_', ' ')} ({out[k]['matched']} of {out['presses']})", out[k]["late_after_due"])
        for k in ("window_message", "directinput")]
    print(f"{'after the due time':40} {'mean':>7} {'sd':>7} {'min':>7} {'p50':>7} {'p99':>7} {'max':>7}  (ms)")
    for label, s in rows:
        if s:
            print(f"{label:40} " + " ".join(f"{s[k]:7.3f}" for k in
                                             ("mean_ms", "sd_ms", "min_ms", "p50_ms", "p99_ms", "max_ms")))
        else:
            print(f"{label:40} none")
    if len(sys.argv) > 3:
        json.dump(out, open(sys.argv[3], "w"), indent=1)
    ok = out["window_message"]["matched"] == out["presses"]
    sys.exit(0 if ok else 1)


main()
