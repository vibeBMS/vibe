#!/usr/bin/env python3
"""Pin the fields of an HFS+ volume (a file) that hold the time it was made or
a random value, so that the same files give the same bytes
(tools/package_dmg.sh; docs/BUILDING.md, "Reproducible builds").

  hfsplus_pin.py <volume> <SOURCE_DATE_EPOCH>

It sets:
- in the volume header and its copy at the end of the volume: the creation
  date (local time, which is UTC here), the modification and checked dates
  to SOURCE_DATE_EPOCH, the backup date to 0 (never), and the volume's 64-bit
  ID (the last 8 bytes of the Finder info) to the first 8 bytes of the
  SHA-256 of the volume with that ID zeroed;
- in each folder and file record of the catalog B-tree: the creation,
  content, attribute and access dates to SOURCE_DATE_EPOCH, the backup date
  to 0.
It stops on anything it does not expect (a catalog in more than its fork's
eight extents, an HFS wrapper, a journal).
"""
import hashlib
import struct
import sys

MAC_EPOCH = 2082844800  # 1904-01-01 to 1970-01-01, in seconds


def main(path, sde):
    t = sde + MAC_EPOCH
    with open(path, "rb") as f:
        v = bytearray(f.read())

    def u16(o):
        return struct.unpack_from(">H", v, o)[0]

    def u32(o):
        return struct.unpack_from(">I", v, o)[0]

    vh = 1024
    if v[vh:vh + 2] != b"H+":
        sys.exit(f"{path}: not an HFS+ volume")
    if u32(vh + 4) & 0x2000:  # kHFSVolumeJournaledMask
        sys.exit(f"{path}: journaled (not handled)")
    block = u32(vh + 40)
    alt = u32(vh + 44) * block - 1024
    if v[alt:alt + 2] != b"H+":
        sys.exit(f"{path}: no volume header copy at {alt}")

    # the catalog: its fork's extents (the overflow file is not read)
    fork = vh + 272
    size, blocks = struct.unpack_from(">Q4xI", v, fork)
    runs = [struct.unpack_from(">II", v, fork + 16 + 8 * i) for i in range(8)]
    if sum(n for _, n in runs) != blocks:
        sys.exit(f"{path}: the catalog has more than eight extents")

    def at(pos):  # the catalog file's byte pos in the volume
        for start, n in runs:
            if pos < n * block:
                return start * block + pos
            pos -= n * block
        sys.exit(f"{path}: beyond the catalog")

    head = at(0)
    first_leaf = u32(head + 14 + 10)
    node_size = u16(head + 14 + 18)
    records = 0
    node = first_leaf
    while node:
        o = at(node * node_size)
        if at(node * node_size + node_size - 1) != o + node_size - 1:
            sys.exit(f"{path}: a catalog node across extents")
        flink = u32(o)
        kind, count = struct.unpack_from(">bxH", v, o + 8)
        if kind != -1:
            sys.exit(f"{path}: catalog node {node} is not a leaf")
        for i in range(count):
            r = o + u16(o + node_size - 2 * (i + 1))
            data = r + 2 + u16(r)
            if u16(data) in (1, 2):  # a folder, a file: their dates at the same places
                struct.pack_into(">IIIII", v, data + 12, t, t, t, t, 0)
                records += 1
        node = flink

    for h in (vh, alt):
        struct.pack_into(">IIII", v, h + 16, t, t, 0, t)
        v[h + 104:h + 112] = bytes(8)
    vid = bytearray(hashlib.sha256(v).digest()[:8])
    for half in (0, 4):  # neither half may be 0
        if not any(vid[half:half + 4]):
            vid[half + 3] = 1
    for h in (vh, alt):
        v[h + 104:h + 112] = vid
    with open(path, "r+b") as f:
        f.write(v)
    print(f"{path}: {records} catalog records and the volume header at {sde}, volume ID {vid.hex()}")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    main(sys.argv[1], int(sys.argv[2]))
