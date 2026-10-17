# CardDeck

A small macOS app (Rust core + Tauri UI) that opens card designs saved as `.card` files by Magicard Hub
and prints them through the **official Magicard macOS CUPS driver**. Personal-use tool, working name
"CardDeck". It is not affiliated with Magicard, has no Magicard branding and ships none of their
artwork or code. "Compatible with" is as far as the naming goes.

What it does (MVP):

* **Viewer**: open a `.card` (dialog, drag and drop, double click in Finder), see front and back at
  full 1013×642 px (300 dpi CR80), zoom, front/back/both. Unsupported things produce warnings instead of errors.
* **Print**: builds a duplex job, saves it as PDF/PNG (dry run, no printer needed) or sends it to a CUPS
  queue with `lp` and the Magicard PPD options.
* Not in the MVP: variables / CSV merge, barcodes other than the stored bitmap, HoloKote, mag stripe,
  chip/HoloPatch holes, a designer.

```
crates/carddeck-core   parser, renderer (tiny-skia + fontdb + rustybuzz), print job, CUPS parsing
crates/carddeck-cli    `carddeck` command line: info / render / job / calibrate / print / printers
src-tauri              Tauri v2 desktop shell (thin: commands + file opening)
ui/                    plain HTML/JS/CSS front end (no npm, no bundler)
docs/                  test-print procedure
```

## Download (unsigned builds)

GitHub Actions (`.github/workflows/build.yml`) builds `CardDeck-arm64.dmg` and `CardDeck-x86_64.dmg`
(plus zipped `.app`) on every push to `main`: open the run, download the artifact.
Apple silicon Mac: take **arm64**. Intel Mac: **x86_64**.

The app is not notarized. First start: **right-click CardDeck.app → Open → Open**. If macOS says the
app is "damaged": `xattr -dr com.apple.quarantine /Applications/CardDeck.app`.

## Install the Magicard driver (macOS, driver 1.5.0)

Download the package for your Mac (`uname -m` prints `arm64` or `x86_64`):

* Apple silicon: <https://storage.googleapis.com/public_drivers/macos/1.5.0/magicard_ltd-mac-os-driver-1.5.0-arm64.pkg>
* Intel: <https://storage.googleapis.com/public_drivers/macos/1.5.0/magicard_ltd-mac-os-driver-1.5.0-x86_64.pkg>

1. Open the `.pkg`. If Gatekeeper complains, right-click → Open, or System Settings → Privacy & Security → "Open Anyway".
2. Connect the printer by USB and switch it on.
3. System Settings → Printers & Scanners → **Add Printer…** → choose the Magicard in the list (driver "Magicard …").
   If it is not offered, add it with a manual driver pick. Magicard's page also describes adding the queue through
   the CUPS web UI (`http://localhost:631`) on macOS versions that still ship it.
4. Check from Terminal:
   ```sh
   lpstat -p                                   # the queue name, e.g. Magicard_300
   lpoptions -p <queue> -l | grep -E 'UTDuplex|CFColourFormat|CFBlackResin|CFOverCoat|CFRotation|PageSize'
   ```
   CardDeck needs these option names. The app checks them for you (Preview shows problems if one is missing).

## First test print checklist

Full procedure with a decision table: [docs/TEST-PRINT.md](docs/TEST-PRINT.md). Short version:

- [ ] Driver installed, printer on, queue visible in CardDeck's printer list (↻ to refresh).
- [ ] Ribbon and a spare blank card loaded.
- [ ] Open **Calibration card**. Press **Preview print pages**: the pages are portrait, red square bottom-left, "TOP" arrow on the left.
- [ ] **Save job (PDF + PNG)…** once and open `job.pdf` in Preview. It must be 153×243 pt, 2 pages.
- [ ] Print **Front only** with default settings. Compare with the on-screen card: red square top-left, TOP arrow at the top edge, text reads left to right (not mirrored, not upside down).
- [ ] Wrong? Change Orientation (Rotate front 180° / Turn direction) and print again until front is right.
- [ ] Print **Back only**; fix with "Rotate back 180°" if needed.
- [ ] Print **Both** (one duplex job). Front and back must both be upright.
- [ ] Black check: the pure-black text and the black square print as resin (crisp, matte black, no colour fringe). "Near black" text prints as composite.
- [ ] Edge check: the thin black frame is visible on all four edges (if one side is cut, see the fit options).
- [ ] Only then print a real card. Write the winning settings down; the app does not remember them yet.

## Build, run, test

Prerequisites: Rust (stable, `rustup`), macOS with Xcode command line tools for the app. The core and CLI build and test on Linux and macOS.

```sh
# core + CLI (any OS)
cargo test --workspace --release
cargo run -p carddeck-cli --release -- info my.card
cargo run -p carddeck-cli --release -- render my.card out/            # front/back PNGs
cargo run -p carddeck-cli --release -- job my.card out/job --sides both   # dry run: job.pdf, page PNGs, command.txt
cargo run -p carddeck-cli --release -- calibrate out/cal                  # calibration card job
cargo run -p carddeck-cli --release -- printers

# desktop app (macOS)
cargo install tauri-cli --version "^2" --locked
cd src-tauri
cargo tauri dev                              # run
cargo tauri build --bundles app,dmg          # unsigned CardDeck.app / .dmg in src-tauri/target/release/bundle
cargo tauri build --target x86_64-apple-darwin --bundles app,dmg   # Intel build on an Apple-silicon Mac
```

Open from the command line: `open -a CardDeck my.card`, or double click a `.card` after setting CardDeck as the default app
(Finder → Get Info → Open with → CardDeck → Change All…). The bundle declares the `.card` extension.

### Tests

* `crates/carddeck-core/tests/synthetic.rs`: 22 tests on generated cards (transforms, fills, strokes, clip,
  text alignment, photo-black nudge, orientation of the print page, PDF structure, `lp` command snapshot, `lpstat`/`lpoptions` parsing).
* `crates/carddeck-core/tests/samples.rs`: numeric comparison against real Hub files. These files are private
  and **not in the repo** (`samples/` is git-ignored). Put `.card` files in `samples/` (or set `CARDDECK_SAMPLES=/path`) and run
  `cargo test --release -p carddeck-core --test samples -- --nocapture`. Without samples the tests print SKIP.
  They compare our render of the front side with the Hub-generated `about.preview` (mean absolute difference per RGB channel, 0..255),
  with and without the text boxes, check both sides against reference renders, and decode every QR code with an independent decoder.

Results with 12 real files (Hub 2.0.6/2.0.8) on Linux with Liberation fonts as substitutes:

| Check | Result |
|---|---|
| front vs Hub `about.preview`, whole image | mean abs diff 6.1 / 255 (worst 6.5): almost all of it is font substitution |
| front vs preview, outside text boxes (shapes, images, QR) | mean 1.4 (worst 3.1; the worst file has a stale preview) |
| both sides vs the Pillow reference renders | front 4.5, back 2.9 |
| stored QR bitmaps decode to their `qrData` | 24/24 |
| QR codes still scan after the full render | 24/24 |
| regenerated QR (if a bitmap is missing) decodes to `qrData` | 24/24 |
| `job.pdf` rasterised at 300 dpi by poppler vs the page PNG | 638×1013, mean diff 0.0 (pixel exact) |

## How printing works

1. Each side is rendered to 1013×642 RGB (transparent card corners become white).
2. Photos: pure `#000000` pixels become `(1,1,1)`. The Magicard driver puts a pixel on the black resin (K) panel only
   if it is exactly RGB 0,0,0, so only text and shapes that you coloured `#000000` print with resin. (Same rule Hub uses.)
3. The landscape bitmap is turned into the driver's portrait raster page (`PageSize 1XCR80` = 153×243 pt = 638×1013 px).
   Default: counter-clockwise 90°, then cropping 2 px from each long edge (no resampling, so pure black stays pure; "Stretch" is the alternative).
4. A lossless PDF is written: one page per side, odd page front, even page back, DeviceRGB, page box 153×243 pt.
   Copies are expanded into the page list so duplex pairing cannot break.
5. `lp -d <queue> -n 1 -o PageSize=1XCR80 -o UTDuplex=1Both -o CFColourFormat=1ColourYMCK -o CFBlackResin=0BlackResinK -o CBColourFormat=… -o CBBlackResin=… -o CFOverCoat=1PrintOvercoat -o CBOverCoat=… -o print-scaling=none job.pdf`

| UI setting | PPD option(s) |
|---|---|
| Sides | `UTDuplex` = `0Front` / `1Both` / `2Back` |
| Black: resin K | `CFColourFormat`/`CBColourFormat` = `1ColourYMCK`, `CFBlackResin`/`CBBlackResin` = `0BlackResinK` |
| Black: composite | `0ColourYMC`, `1BlackResinYMC` |
| Black: mono K | `2MonoK`, `0BlackResinK` |
| Overcoat | `CFOverCoat` / `CBOverCoat` = `1PrintOvercoat` / `0PrintNoOvercoat` |
| Advanced extra options | any `NAME=VALUE`, for example `CFRotation=1Rotation` (the driver's own 180° option) |

The option names come from the GPL Magicard driver PPDs (`pro300.ppd`) and the macOS driver package ships the same names;
the real PPD of the installed queue is checked with `lpoptions -p <queue> -l` when you preview.

## Fonts

Cards only store a font family. Designs seen so far use Myriad Pro, Minion Pro and Georgia, which macOS does not have.
Fallbacks (first installed wins): Myriad Pro → Helvetica Neue → Helvetica → Arial; Minion Pro → Times New Roman → Times → Georgia;
Georgia → Georgia → Times New Roman; Arial → Arial → Helvetica Neue; Courier New → Courier New → Courier → Menlo.
Because Myriad Pro is about 10 % narrower than Helvetica, substituted text is squeezed horizontally (Myriad 90 %; up to 75 % if a box
would otherwise overflow or lose lines). If you own the real fonts, install them and they are used automatically. Override per design font with
`~/Library/Application Support/dev.carddeck.desktop/fonts.json`:

```json
{ "myriad pro": ["Avenir Next", "Helvetica Neue"], "minion pro": ["Charter", "Times New Roman"] }
```

Extra font files can be dropped in `~/Library/Application Support/dev.carddeck.desktop/fonts/`.

## Known limitations

* **Unsigned app** (ad-hoc signature, not notarized): right-click → Open on first launch.
* **Orientation of the raster page is UNVERIFIED** on real hardware. The default (counter-clockwise, front and back alike) is derived by reading the
  GPL driver source, nothing more. Rotation and turn direction are visible settings; calibrate once with the test-print procedure.
* **Fonts**: substitutes differ in shape from Myriad/Minion; line breaks and text widths can differ from Hub. Colours and shapes match closely, text does not match exactly.
* The driver does colour correction itself; Hub's `adjustments` block (gamma, RGB multipliers) is ignored. macOS colour management in the PDF-to-raster
  filter could in theory alter exact `0,0,0`; the black check in the test-print checklist catches that.
* Not handled: variable/CSV data, barcode regeneration (the stored bitmap is drawn), HoloKote, HoloPatch, mag stripe, chip/slot overcoat holes
  (flags in the file only raise a warning), dark-card specifics, Xtended/CR79 page sizes (CR79 and Xtended render but the print page is fixed to CR80),
  image/gradient fills are implemented from code reading and have never been checked against a real file.
* Rendering with a different engine than Hub's WebView: sub-pixel differences at shape edges (mean error about 1 of 255 outside text).
* Settings are not persisted between launches.
* The app was only compiled and run headless on Linux; the macOS-specific bits (file association, `Opened` event, dialogs) are built by CI and untested until you run them.

## Legal / provenance

Clean-room: the renderer and print code were written from observed behaviour of real files and the public driver option names.
No Hub code, assets or sample cards are included. The Magicard driver is GPL and is only *used* (via CUPS), never linked or copied.
