# Colour correction (preview) in CardDeck

Hub shows a **Colour Correct** drop-down and six sliders (Brightness, Contrast, Saturation, Red, Green, Blue) on the
Adjustments panel and stores them in the `.card` file. CardDeck reproduces the **on-screen preview** of those
settings, saves them back into the same block, and tells the printer driver the matching option when printing.

## What happens where

| | On screen | Print from the app |
|---|---|---|
| Colour Correct, LUT modes (Perceptual, Saturation, Relative, Absolute) | corrected on screen | sent as PPD option `ColourCorrection` (table below); the bitmap is **not** given that table |
| Colour Correct, Gamma 1 / Gamma 2 | corrected on screen | **not** baked. The driver gets `ColourCorrection=1ColourCorrection` (Driver managed). Baking the preview curve and sending None printed light, washed-out cards |
| Sliders (B/C/S/R/G/B) | corrected on screen | baked into the bitmap (the driver has no such options), then the driver still applies the Colour Correct mode above |

The LUT modes are never baked: the driver applies its own table, so a pre-corrected bitmap would be corrected twice.
Gamma is not baked either. On a Magicard 300, `ColourCorrection=None` makes the driver skip its colour match, and a card printed that way (Gamma 2, compared with Hub) came out light and washed out. Driver managed is the option the driver actually colour-matches with. Gamma 1 and Gamma 2 share it; only the on-screen preview differs.
The CLI leaves sliders unbaked unless you pass `--bake-adjustments` (then it matches the app).

## Mode to PPD mapping

Values from `pro300.ppd`, `pro300neo.ppd`, `pro360.ppd`, `pro360NEO.ppd` (Magicard Linux driver 1.4.0; the macOS
1.5.0 queues are expected to be identical; check with `lpoptions -p <queue> -l | grep ColourCorrection`).
`ConvertColourCorrectionValue` in the driver's `utils.c` maps the number to its internal mode.

| Hub / CardDeck mode | `.card` JSON value | Sent to `lp` | Driver does | Exact? |
|---|---|---|---|---|
| None | `None` | `-o ColourCorrection=0ColourCorrection` | nothing | yes |
| Gamma 1 | `Gamma1` | `-o ColourCorrection=1ColourCorrection` (Driver managed) | its legacy internal table (`ICC_Internal`) | **no**, closest option |
| Gamma 2 | `Gamma2` | `-o ColourCorrection=1ColourCorrection` (Driver managed) | same | **no**, closest option |
| Perceptual | `Perceptual` | `-o ColourCorrection=2ColourCorrection` | MAGIR2X, perceptual | yes |
| Saturation | `Saturation` | `-o ColourCorrection=3ColourCorrection` | MAGIR2X, saturation | yes |
| Relative Colorimetric | `RelativeColorimetric` | `-o ColourCorrection=4ColourCorrection` | MAGIR2X, relative | yes |
| Absolute Colorimetric | `AbsoluteColorimetric` | `-o ColourCorrection=5ColourCorrection` (PPD default) | MAGIR2X, absolute | yes |

There is no Gamma choice in the driver. Hub's own file converter turns the old `ICC_Internal` value into `Gamma1`,
so "Driver managed" is the closest analogue; it is not the same curve as the preview. The app sends that option
and does not bake the preview curve. A print that baked the curve and sent `ColourCorrection=0ColourCorrection`
was light and washed out next to the same card printed from Hub with Gamma 2.

## How the preview is computed

Order (as observed in Hub): RGB multipliers, brightness (`+255*b`), contrast (factor `((c+100)/100)^2` around 128),
saturation (Rec. 601 luma kept, chroma `*2^s`), then the Colour Correct mode. Every stage clamps and rounds to 8 bits.
Slider ranges: brightness +-0.5, contrast +-15, saturation +-5, RGB 0.5..1.5. Hub's sharpen filter is **not** implemented.

* **Perceptual / Saturation / Relative / Absolute**: the driver's MAGIR2X table (65x65x65 grid, step 4, one
  cyan/magenta/yellow byte per intent) with the driver's interpolation; the preview colour is `255 - ink`.
  Exact port. Pure `#000000` is exempt (it prints on the K resin panel, and Hub's preview leaves it alone too).
  **GPL**: see `LICENSE-GPL-NOTES.md`.
* **Gamma 1 / Gamma 2**: Hub does these in a closed DLL, so they are **fitted**: per channel
  `255 * clamp(c * (x/255)^g + b)` with
  Gamma 1 `g=1.5231 c=1.0049 b=-0.0053`, Gamma 2 `g=1.4958 c=1.0578 b=-0.0569`.
  White stays white; mid tones get darker. Hub's default for a file without an `adjustments` block is Gamma 1.

## How good is it? (against Hub's own preview)

Ground truth: 7 screenshots of Hub (None, Gamma 1, Gamma 2, Absolute, Relative, Perceptual, Saturation) showing the
same two-sided card; 30 swatches each (front 16, back 14; two back swatches hidden by an overlay were dropped). The
screenshots were taken on a Display P3 monitor, so they were converted to sRGB first (fitted matrix, mean error 0.24).
Per-channel absolute error in sRGB 0..255, `tools/colour-fit/report.py`:

| Setting | mean | max | RMSE |
|---|---|---|---|
| None | 0.16 | 2 | 0.45 |
| Gamma 1 (fitted) | 0.21 | 2 | 0.48 |
| Gamma 2 (fitted) | 0.29 | 2 | 0.60 |
| Absolute Colorimetric | 0.21 | 1 | 0.46 |
| Relative Colorimetric | 0.22 | 1 | 0.47 |
| Perceptual | 0.18 | 1 | 0.42 |
| Saturation | 0.23 | 3 | 0.57 |

The None row is the floor (screenshot and colour-management noise). LUT modes reach it, so they are exact up to
noise. Gamma 1/2 are in-sample fits; fitting on the front-side swatches and testing on the back (and vice versa) gave
mean 0.18 to 0.4, max 2. Caveats: the fit data come from one palette (sparse near pure black and white), and the
fixture was derived from one private card. The automated tests (`crates/carddeck-core/tests/colour.rs`) use the
frozen fixture with tolerances: None mean 0.5/max 2, LUT modes mean 0.6/max 3, Gamma mean 0.8/max 4.

## Saving

The mode and sliders are stored in the file's top-level `adjustments` object using Hub's keys:
`correction, brightness, contrast, saturation, red, green, blue`. Only that value is edited, textually, so the rest
of the file keeps its bytes (key order, indentation, line endings, BOM, embedded images). Writes are atomic
(temp file + rename) and checked: the parsed JSON before and after must differ only in `adjustments`. A file
without the block gets one appended. `correction` is written with Hub's enum names (`Gamma1`, `RelativeColorimetric`, ...);
display labels with spaces ("Gamma 1") and the legacy `ICC_Internal/External` names are still read.
