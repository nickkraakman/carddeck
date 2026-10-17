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

CardDeck needs those option names. Preview will warn if a chosen queue is not a Magicard driver.

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
   - Set **Sides** and **Copies**.
   - Leave **Overcoat** on unless you intentionally want it off.
   - Set **Colour correct** — CardDeck loads this from the file’s `adjustments.correction` (same field Hub stores). Changing it writes back into that `.card` file.
5. Optional: **Preview print pages** to see the 638×1013 portrait rasters the driver receives, or **Save job (PDF + PNG)…** for a dry run with no printer.
6. Click **Print…** and confirm. That sends `lp` with the Magicard PPD options (uses a blank card + ribbon).

First hardware run: use **Calibration card** and the checklist in [docs/TEST-PRINT.md](docs/TEST-PRINT.md).

### Finder “Open with”

Get Info on a `.card` → Open with → CardDeck → **Change All…**. After that, double-click opens in CardDeck. The app also registers the `.card` extension in its bundle.

## What CardDeck does (and does not)

* **Does:** view `.card` front/back at 300 dpi CR80, build duplex print jobs, talk to the official Magicard CUPS driver, honour Hub’s stored **Colour correct** setting.
* **Does not (yet):** designer, CSV/variables, HoloKote, mag stripe, chip/HoloPatch holes, brightness/contrast/RGB sliders from Hub’s Adjustments panel (those numbers are in the file but not applied).

```
crates/carddeck-core   parser, renderer, print job, CUPS parsing
crates/carddeck-cli    headless CLI: info / render / job / calibrate / print / printers
src-tauri              Tauri v2 desktop shell
ui/                    plain HTML/JS/CSS (no npm)
docs/                  test-print procedure
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
* Hub brightness / contrast / saturation / RGB multipliers in `adjustments` are not applied; only **Colour correct** is sent to the driver (`ColourCorrection` PPD option).
* Not handled: variable data, barcode regeneration, HoloKote / HoloPatch / mag / chip cut-outs, dark-card specifics, non-CR80 page sizes for printing.

## Legal / provenance

Clean-room: renderer and print code from observed `.card` behaviour and public Magicard driver option names.
No Hub code, assets, or sample cards are in this repository. The Magicard driver is GPL and is only *used* via CUPS.
