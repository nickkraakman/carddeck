#!/usr/bin/env python3
"""Extracts the MAGIR2X colour table from the GPL Magicard Linux driver source into a compressed blob.

Input : magicard_ltd-linux_driver-1.4.0/src/colour-profiles/magir2x.c   (GPL-2.0-or-later, (c) Magicard Ltd)
Output: crates/carddeck-core/src/colour/magir2x.lut.z

Blob format (after zlib inflate): 12 planes of 65*65*65 bytes, plane p holds byte p of the C array's
12-byte grid entries:  planes 0..3 cyan for the intents perceptual, saturation, relative colorimetric,
absolute colorimetric; planes 4..7 magenta (same order); planes 8..11 yellow (same order).
Inside a plane the index is [r][g][b] (r slowest, b fastest) and every value is stored as the
difference (mod 256) to the previous blue index (first blue index stored as is). This only makes the
blob compress better (about 0.9 MB instead of 2.6 MB); `colour::lut` undoes it.
Grid step is 4 (input value = index * 4, last index 64 = 256). The 12 byte header of the C array
(grid step = 4, then 8 more bytes) and the zero padding after the grid are dropped.
The result is GPL-licensed data: see LICENSE-GPL-NOTES.md.

Usage: tools/gen-lut.py /path/to/magir2x.c
"""
import re, sys, zlib, pathlib, struct

src = pathlib.Path(sys.argv[1]).read_text()
body = src[src.index("{", src.index("MAGIR2X")) + 1 : src.rindex("}")]
data = bytes(int(x) for x in re.findall(r"\d+", body))
hdr = struct.unpack("<I", data[:4])[0]
assert hdr == 4, f"unexpected grid step {hdr}"
grid = data[12 : 12 + 65 ** 3 * 12]
assert len(grid) == 65 ** 3 * 12
assert not any(data[12 + 65 ** 3 * 12 :]), "unexpected non-zero data after the grid"
import numpy as np
a = np.frombuffer(grid, dtype=np.uint8).reshape(65, 65, 65, 12)
d = a.astype(np.int16)
d[:, :, 1:, :] = a[:, :, 1:, :].astype(np.int16) - a[:, :, :-1, :]
planar = np.moveaxis((d & 0xFF).astype(np.uint8), 3, 0)
out = pathlib.Path(__file__).resolve().parent.parent / "crates/carddeck-core/src/colour/magir2x.lut.z"
out.write_bytes(zlib.compress(planar.tobytes(), 9))
print(out, out.stat().st_size, "bytes (from", len(grid), ")")
