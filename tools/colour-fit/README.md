# How the Gamma 1/2 curves were fitted

Scratch scripts, kept for reproducibility. They are **not** part of the build and expect local paths and
screenshots of Magicard Hub that are not in the repository (the screenshots show a private card).

* `register.py`, `extract.py`, `sample.py`: locate the card in each screenshot and read the swatch colours.
* `display.py`: Display P3 -> sRGB (the screenshots were taken on a P3 monitor; fitted 3x3 matrix in linear light).
* `lut.py`: reference implementation of the driver LUT interpolation, used to cross-check the Rust port.
* `algo.py`, `evalfit.py`: the Hub-style pipeline and the Gamma curve fit `255*clip(c*(x/255)^g + b)`.
* `report.py`: per-setting error table (see `docs/COLOUR-CORRECTION.md`).

The numbers they produced are frozen in `crates/carddeck-core/tests/fixtures/hub-colour-swatches.csv`.
