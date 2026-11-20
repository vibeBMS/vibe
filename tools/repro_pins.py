#!/usr/bin/env python3
"""The pins of tools/repro.env that the Toolchain pins workflow moves
(.github/workflows/toolchain-pins.yml; docs/BUILDING.md, "Reproducible builds").

  repro_pins.py choose <repro.env> <facts> <update out> <major out>
  repro_pins.py check <base repro.env> <head repro.env> [<head linux-arm64.lock>]

choose: <facts> has "<name>=<value>" lines, what the workflow found:
  macos_newest, windows_newest   the newest GA runner labels
  xcode@<label>                  the Xcode releases (major.minor) on that
                                 label's image
  msvc@<label>, sdk@<label>      the side-by-side MSVC toolsets (installer
                                 components, e.g. 14.44.17.14) and Windows
                                 SDKs on it
  <PIN>=<value>                  a new value for any other pin (an image's
                                 digest, UBUNTU_SNAPSHOT, the Android tools,
                                 LIBDMG_HFSPLUS_COMMIT)
A value that does not look like one for its pin (a version is numbers and
dots, an image <repository>[:<tag>]@sha256:<digest>, a commit 40 hex digits)
is left out, with a warning on stderr.
It writes two copies of <repro.env>: <update out> with the changes that are
merged once CI passes (newer tools on the same runner labels, the same Xcode
major), <major out> with those held for a person (a new runner label and its
tools, a new major version of Xcode, the NDK, cargo-ndk, the JDK, Gradle or
the build-tools).
Each says what it changes on stdout.

check: whether <head> changes only the values of pins (no other line), and
which changes are held: exit 0 when none is, 3 when one is (each said on
stdout), 1 when <head> changes anything else or gives a pin a value that does
not look like one for it. With a lock (tools/build_deps.sh
lock linux-arm64), also that it is one, for <head>'s image and packages. The
Toolchain pins workflow's merge job runs it on its update.
"""
import re
import sys

LABELS = {"MACOS_RUNNER", "WINDOWS_RUNNER"}
# a change of the first number waits for a person
MAJOR = {"XCODE_VERSION", "NDK_VERSION", "CARGO_NDK_VERSION", "JAVA_VERSION", "GRADLE_VERSION",
         "BUILD_TOOLS_VERSION"}
# versions that only move forward
VERSIONS = MAJOR | {"MSVC_COMPONENT", "WINDOWS_SDK", "UBUNTU_SNAPSHOT"}
# images: the digest moves, the repository and tag stay
IMAGES = {"LINUX_IMAGE_X64", "LINUX_IMAGE_ARM64", "PORTABLE_IMAGE_X64", "PORTABLE_IMAGE_ARM64", "WEB_IMAGE"}
# commits of a repository built from source: any other full commit
COMMITS = {"LIBDMG_HFSPLUS_COMMIT"}
MOVED = LABELS | VERSIONS | IMAGES | COMMITS
# what a value of each looks like. The registries' answers are text from the
# network and tools/repro.env is sourced by bash: nothing else is written
# there or merged
SHAPE = {
    **{k: r"[0-9]+(\.[0-9]+)*" for k in VERSIONS},
    "UBUNTU_SNAPSHOT": r"[0-9]{8}T[0-9]{6}Z",
    **{k: r"(macos|windows)-[0-9]+(-[a-z0-9]+)*" for k in LABELS},
    **{k: r"[a-z0-9]+([._/-][a-z0-9]+)*(:[A-Za-z0-9_][A-Za-z0-9._-]*)?@sha256:[0-9a-f]{64}" for k in IMAGES},
    **{k: r"[0-9a-f]{40}" for k in COMMITS},
}

LINE = re.compile(r'^([A-Z][A-Z0-9_]*)=(.*)$')


def parse(text):
    pins = {}
    for line in text.splitlines():
        m = LINE.match(line)
        if m:
            v = m.group(2)
            pins[m.group(1)] = v[1:-1] if len(v) >= 2 and v[0] == v[-1] == '"' else v
    return pins


def with_values(text, changes):
    out = []
    for line in text.splitlines(keepends=True):
        m = LINE.match(line.rstrip("\n"))
        if m and m.group(1) in changes:
            v = changes[m.group(1)]
            if m.group(2).startswith('"') or " " in v:
                v = f'"{v}"'
            line = f"{m.group(1)}={v}\n"
        out.append(line)
    return "".join(out)


def well_formed(key, value):
    return re.fullmatch(SHAPE[key], value) is not None


def nums(v):
    return [int(x) for x in re.findall(r"\d+", v)]


def held(key, old, new):
    """Why the change waits for a person, or None."""
    if key in LABELS:
        return f"{key}: a new runner image ({old} -> {new})"
    if key in VERSIONS and nums(new) < nums(old):
        return f"{key}: an older version ({old} -> {new})"
    if key in MAJOR and nums(new)[:1] != nums(old)[:1]:
        return f"{key}: a new major version ({old} -> {new})"
    if key in IMAGES and old.split("@")[0] != new.split("@")[0]:
        return f"{key}: another image, not another digest ({old} -> {new})"
    return None


def check(base, head):
    """(changes, held reasons, errors) of head against base."""
    b, h = parse(base), parse(head)
    changes = {k: h[k] for k in h if b.get(k) != h[k]}
    errors = [f"{k}: removed" for k in b if k not in h]
    errors += [f"{k}: not a pin the workflow moves" for k in changes if k not in MOVED or k not in b]
    errors += [f"{k}: not a value for it: {changes[k]!r}" for k in changes
               if k in MOVED and not well_formed(k, changes[k])]
    if not errors and with_values(base, changes) != head:
        errors.append("lines other than the pins' values differ")
    reasons = [r for k in changes if k in b for r in [held(k, b[k], changes[k])] if r]
    return changes, reasons, errors


LOCK_LINE = re.compile(
    r"[a-z0-9][a-z0-9+.-]*:(arm64|all) [0-9A-Za-z.+~:-]+ [0-9a-f]{64} "
    r"pool/(main|universe)/[a-z0-9+.-]+/[a-z0-9+.-]+/[A-Za-z0-9+.~_-]+\.deb "
    r"(-|http://launchpadlibrarian\.net/\d+/[A-Za-z0-9+.~_%-]+\.deb)")


def check_lock(head, lock):
    """Errors of a lock for head's pins."""
    pins, errors, lines = parse(head), [], 0
    for line in lock.splitlines():
        if line.startswith("# image: ") and line[9:] != pins.get("LINUX_IMAGE_ARM64"):
            errors.append("the lock is for another image than LINUX_IMAGE_ARM64")
        elif line.startswith("# packages: ") and line[12:] != pins.get("LINUX_PACKAGES"):
            errors.append("the lock is for other packages than LINUX_PACKAGES")
        elif not line.startswith("#"):
            lines += 1
            if not LOCK_LINE.fullmatch(line):
                errors.append(f"not a lock's line: {line[:120]}")
    if not lines or "# image: " not in lock or "# packages: " not in lock:
        errors.append("not a lock (tools/build_deps.sh lock linux-arm64)")
    return errors


def newest(versions):
    return max(versions, key=nums) if versions else None


def choose(text, facts):
    pins = parse(text)
    update, major = {}, {}

    def ok(key, value):
        if not well_formed(key, value):
            print(f"::warning::{key}: not a value for it, left out: {value!r}", file=sys.stderr)
            return False
        return True

    def found(name, key):
        return [v for v in facts.get(name, "").split() if ok(key, v)]

    def put(key, value, into=None):
        if value and value != pins.get(key) and ok(key, value):
            (into if into is not None else (major if held(key, pins[key], value) else update))[key] = value

    # Xcode: the newest of its major on the label; a newer major, or the
    # newest label (with its newest Xcode), waits
    mac, xcode = pins["MACOS_RUNNER"], pins["XCODE_VERSION"]
    have = found(f"xcode@{mac}", "XCODE_VERSION")
    put("XCODE_VERSION", newest([v for v in have if nums(v)[:1] == nums(xcode)[:1]]))
    if have and nums(newest(have))[:1] != nums(xcode)[:1]:
        put("XCODE_VERSION", newest(have), major)
    label = facts.get("macos_newest", mac)
    if label != mac and ok("MACOS_RUNNER", label):
        have = found(f"xcode@{label}", "XCODE_VERSION")
        if have:
            major["MACOS_RUNNER"] = label
            major["XCODE_VERSION"] = newest(have)
    # MSVC and the SDK: the newest on the label; the newest label waits
    win = pins["WINDOWS_RUNNER"]
    put("MSVC_COMPONENT", newest(found(f"msvc@{win}", "MSVC_COMPONENT")))
    put("WINDOWS_SDK", newest(found(f"sdk@{win}", "WINDOWS_SDK")))
    label = facts.get("windows_newest", win)
    if label != win and ok("WINDOWS_RUNNER", label):
        msvc, sdk = found(f"msvc@{label}", "MSVC_COMPONENT"), found(f"sdk@{label}", "WINDOWS_SDK")
        if msvc and sdk:
            major["WINDOWS_RUNNER"] = label
            major["MSVC_COMPONENT"] = newest(msvc)
            major["WINDOWS_SDK"] = newest(sdk)
    # the rest as found
    for key in sorted((MOVED - LABELS) & facts.keys()):
        if key in pins and key not in {"XCODE_VERSION", "MSVC_COMPONENT", "WINDOWS_SDK"}:
            put(key, facts[key])
    return with_values(text, update), with_values(text, major), update, major


def main(argv):
    if len(argv) == 6 and argv[1] == "choose":
        text = open(argv[2]).read()
        facts = {}
        for line in open(argv[3]):
            if "=" in line:
                k, v = line.rstrip("\n").split("=", 1)
                facts[k.strip()] = v.strip()
        up, ma, upd, maj = choose(text, facts)
        open(argv[4], "w").write(up)
        open(argv[5], "w").write(ma)
        pins = parse(text)
        for name, changes in (("update", upd), ("major", maj)):
            for k, v in changes.items():
                print(f"{name}\t{k}\t{pins[k]}\t{v}")
        # (the update must pass check, or the merge job would hold it)
        _, reasons, errors = check(text, up)
        assert not reasons and not errors, (reasons, errors)
        return 0
    if len(argv) in (4, 5) and argv[1] == "check":
        head = open(argv[3]).read()
        changes, reasons, errors = check(open(argv[2]).read(), head)
        if len(argv) == 5:
            errors += check_lock(head, open(argv[4]).read())
        for e in errors:
            print(f"error: {e}")
        if errors:
            return 1
        for k, v in changes.items():
            print(f"{k} = {v}")
        for r in reasons:
            print(f"held: {r}")
        return 3 if reasons else 0
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
