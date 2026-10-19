# CardDeck

A small macOS app (Rust core + Tauri UI) that opens card designs saved as `.card` files by Magicard Hub
and prints them through the **official Magicard macOS CUPS driver**. Personal-use tool — not affiliated
with Magicard, ships none of their artwork or code.

## Download (latest unsigned builds)

Every push to `main` builds unsigned arm64 and x86_64 apps:

**[Latest builds on GitHub Actions →](https://github.com/nickkraakman/carddeck/actions/workflows/build.yml?query=branch%3Amain)**

1. Open the newest green run on `main`.
2. Scroll to **Artifacts**.
3. Download:
   - **Apple silicon** → `CardDeck-macOS-arm64` → contains `CardDeck-arm64.dmg` and `CardDeck-arm64.app.zip`
   - **Intel Mac** → `CardDeck-macOS-x86_64` → contains `CardDeck-x86_64.dmg` and `CardDeck-x86_64.app.zip`

Check your Mac: `uname -m` prints `arm64` or `x86_64`.

## Allow the unsigned app (macOS Gatekeeper)

The app is **ad-hoc signed, not notarized**. Gatekeeper will block a normal double-click the first time.

**Preferred:**

1. Open the `.dmg` (or unzip the `.app.zip`).
2. Drag **CardDeck.app** to Applications (or leave it where you like).
3. **Right-click** (or Control-click) **CardDeck.app** → **Open** → **Open**.

**If macOS still blocks it:**

1. Try to open CardDeck once (so the block is recorded).
2. Open **System Settings → Privacy & Security**.
3. Scroll to the security section near the bottom — you should see a message that CardDeck was blocked.
4. Click **Open Anyway**, then confirm **Open**.

**If macOS says the app is “damaged”** (quarantine attribute):

```sh
xattr -dr com.apple.quarantine /Applications/CardDeck.app
```

Then right-click → Open again.

## Install the Magicard macOS driver (1.5.0)

Install the official Magicard CUPS driver **before** printing. Pick the package for your CPU:

| Mac | Package |
|---|---|
| Apple silicon (`arm64`) | [magicard_ltd-mac-os-driver-1.5.0-arm64.pkg](https://storage.googleapis.com/public_drivers/macos/1.5.0/magicard_ltd-mac-os-driver-1.5.0-arm64.pkg) |
| Intel (`x86_64`) | [magicard_ltd-mac-os-driver-1.5.0-x86_64.pkg](https://storage.googleapis.com/public_drivers/macos/1.5.0/magicard_ltd-mac-os-driver-1.5.0-x86_64.pkg) |

1. Open the `.pkg`. If Gatekeeper complains: right-click → Open, or **System Settings → Privacy & Security → Open Anyway**.
2. Connect the printer by USB and switch it on.
3. **System Settings → Printers & Scanners → Add Printer…** → choose the Magicard (driver name starts with “Magicard”).
4. Confirm from Terminal:

```sh
lpstat -p
lpoptions -p <queue> -l | grep -E 'UTDuplex|CFColourFormat|CFBlackResin|CFOverCoat|ColourCorrection|PageSize'
```

CardDeck needs those option names. A queue that is not a Magicard driver will reject the options it does not know.

## Install Minion Pro and Myriad Pro

Hub designs usually name **Myriad Pro** and **Minion Pro**. macOS does not ship them. Without them CardDeck
falls back (Helvetica Neue / Times, etc.) and text spacing will not match Hub.

1. Obtain the fonts legally (Adobe Fonts / your Creative Cloud entitlement, or whatever license you already have).
2. Install them for your user, either:
   - **Font Book** → File → Add Fonts… → select the `.otf` / `.ttf` files, or
   - Copy the font files into `~/Library/Fonts/`.
3. Quit and reopen CardDeck so it rescans fonts.

Optional overrides (if you want a different substitute family) go in:

`~/Library/Application Support/dev.carddeck.desktop/fonts.json`

```json
{
  "myriad pro": ["Avenir Next", "Helvetica Neue"],
  "minion pro": ["Charter", "Times New Roman"]
}
```

You can also drop extra font files in `~/Library/Application Support/dev.carddeck.desktop/fonts/`.

## Print a `.card` file

1. Install the Magicard driver and add the printer (above).
2. Open CardDeck → **Open…** (or drop a `.card` onto the window, or double-click a `.card` after setting CardDeck as the default app).
3. Check front/back in the viewer. Warnings at the top mean something was skipped or approximated.
4. In the Print panel:
   - Choose the Magicard **Printer** (↻ to refresh).
   - Set **Sides** and **Copies**. Overcoat is always applied to both sides.
   - Colour is set in the **Colour** panel above Print (see below). The chosen Colour Correct mode is sent to the printer as the matching `ColourCorrection` option. Sliders and Gamma 1/2 are applied to the printed bitmap as well.
5. Click **Print…** and confirm. That sends `lp` with the Magicard PPD options (uses a blank card + ribbon).
   A dry run without the app: `carddeck job my.card out/job` writes `job.pdf`, page PNGs and `command.txt`.

### Colour correction

The **Colour** panel has Hub's **Colour Correct** drop-down (None, Gamma 1, Gamma 2, Absolute / Relative
Colorimetric, Perceptual, Saturation) and the Brightness / Contrast / Saturation / Red / Green / Blue sliders. They start
from the card's saved `adjustments`; **Reset** returns to Hub's defaults (Gamma 1, sliders neutral). Changes show in the
viewer immediately and are written back into the `.card` file's `adjustments` block. **Saved to file** flashes with the time of each write.

The Colour Correct mode goes to the driver as a PPD option. Perceptual / Saturation / Relative / Absolute are **not**
also applied to the bitmap (the driver would correct them twice). Sliders and Gamma 1/2 have no driver options, so the
app always bakes those into the bitmap; for Gamma the driver then gets `ColourCorrection=0ColourCorrection`.

| Colour Correct | What the app sends |
|---|---|
| None | `ColourCorrection=0ColourCorrection` |
| Gamma 1 / Gamma 2 | curve baked into the bitmap, then `ColourCorrection=0ColourCorrection` |
| Perceptual | `ColourCorrection=2ColourCorrection` |
| Saturation | `ColourCorrection=3ColourCorrection` |
| Relative Colorimetric | `ColourCorrection=4ColourCorrection` |
| Absolute Colorimetric | `ColourCorrection=5ColourCorrection` |

Details, fit method and error numbers: [docs/COLOUR-CORRECTION.md](docs/COLOUR-CORRECTION.md).
CLI: `carddeck adjust my.card --correction Perceptual --saturation 1`. A CLI print matches the app's bake only with `--bake-adjustments`.

First hardware run: use **Calibration card** and the checklist in [docs/TEST-PRINT.md](docs/TEST-PRINT.md).

### Finder “Open with”

Get Info on a `.card` → Open with → CardDeck → **Change All…**. After that, double-click opens in CardDeck. The app also registers the `.card` extension in its bundle.

## What CardDeck does (and does not)

* **Does:** view `.card` front/back at 300 dpi CR80, build duplex print jobs, talk to the official Magicard CUPS driver, preview and edit Hub’s **Colour Correct** and Brightness/Contrast/Saturation/RGB adjustments (saved back to the file), and send the matching `ColourCorrection` option to the driver.
* **Does not (yet):** designer, CSV/variables, HoloKote, mag stripe, chip/HoloPatch holes, Hub’s sharpen filter.

```
crates/carddeck-core   parser, renderer, print job, CUPS parsing
crates/carddeck-cli    headless CLI: info / render / job / calibrate / print / printers
src-tauri              Tauri v2 desktop shell
ui/                    plain HTML/JS/CSS (no npm)
docs/                  test-print procedure, colour-correction notes
tools/                 gen-lut.py (colour table repack), colour-fit/ (how Gamma 1/2 were fitted)
```

## Build from source

```sh
# core + CLI (any OS)
cargo test --workspace --release
cargo run -p carddeck-cli --release -- info my.card

# desktop app (macOS)
cargo install tauri-cli --version "^2" --locked
cd src-tauri
cargo tauri dev
cargo tauri build --bundles app,dmg
cargo tauri build --target x86_64-apple-darwin --bundles app,dmg   # Intel build on Apple silicon
```

## Known limitations

* Unsigned / not notarized — see Gatekeeper steps above.
* Raster page orientation defaults are derived from the public driver; calibrate once on real hardware ([docs/TEST-PRINT.md](docs/TEST-PRINT.md)).
* Font substitutes change text metrics vs Hub unless Myriad Pro / Minion Pro are installed.
* Colour: the preview of Perceptual / Saturation / Relative / Absolute is exact (the driver’s own table) and that table is applied by the driver at print time. **Gamma 1/2 are fitted approximations** of Hub and are baked into the printed bitmap. Sliders are baked in too (the driver has no slider options); that can move pure black off `#000000`, so resin K may not catch it. Hub’s sharpen is not reproduced. See [docs/COLOUR-CORRECTION.md](docs/COLOUR-CORRECTION.md).
* Not handled: variable data, barcode regeneration, HoloKote / HoloPatch / mag / chip cut-outs, dark-card specifics, non-CR80 page sizes for printing.

## Legal / provenance

Clean-room: renderer and print code from observed `.card` behaviour and public Magicard driver option names.
No Hub code, assets, or sample cards are in this repository. The Magicard driver is GPL and is only *used* via CUPS, **except** the colour table for the Perceptual / Saturation / Relative / Absolute preview, which is taken from the GPL Linux driver and makes the built app GPL-2.0-or-later. See [LICENSE-GPL-NOTES.md](LICENSE-GPL-NOTES.md) (the repository has no `LICENSE` file yet; GPL-2.0-or-later is recommended).
