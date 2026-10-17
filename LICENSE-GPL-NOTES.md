# Licence notes: the colour tables are GPL

This repository currently has **no `LICENSE` file**. Please read this before choosing one.

`crates/carddeck-core/src/colour/magir2x.lut.z` contains the **MAGIR2X colour table** from the Magicard
Linux CUPS driver (`src/colour-profiles/magir2x.c`, package `magicard_ltd-linux_driver-1.4.0`). The package is
distributed under the **GNU General Public License, version 2 or (at your option) any later version**
(see the licence header of `rastertoultra.c`, which `magir2x.c` is part of; the table file itself has no header).
`crates/carddeck-core/src/colour/lut.rs` implements the driver's `ColourCorrectHelix` interpolation.
`tools/gen-lut.py` repacks the table (deltas + zlib) and nothing else.

Consequences:

* `lut.rs`, `magir2x.lut.z`, and any binary built with them (the CLI, the macOS app) are
  **GPL-2.0-or-later** works. Distributing that binary (for example the unsigned builds on GitHub Actions) means
  offering the matching source under the GPL, which a public repository does.
* The rest of the code was written without copying Hub or driver code, so it could be under another licence on its
  own, but the combined work is GPL. The simplest consistent choice is to license the **whole repository as
  GPL-2.0-or-later** and add a `LICENSE` file with the GPL text. (Recommended.)
* If you prefer a permissive licence, remove the four LUT modes: delete the `colour::lut` module and the blob and
  let Perceptual/Saturation/Relative/Absolute fall back to "no preview". Printing is unaffected, because the
  driver does these corrections itself; only the on-screen preview of those four modes depends on the table.
* Nothing from Magicard Hub is included: no Hub code, no `ColourProfiles` DLLs, no Hub assets. Gamma 1/2 are
  curves fitted to observed Hub output.

This is not legal advice.
