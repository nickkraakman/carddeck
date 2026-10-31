# Test-print procedure (orientation and black calibration)

Why: the raster page orientation and the duplex back-side orientation cannot be verified without hardware.
This procedure finds the right settings with 3–5 spare cards. Nothing here needs Nick's designs.

## Before printing (no card used)

1. From the repo: `cargo run -p carddeck-cli --release -- calibrate /tmp/carddeck-cal`. Open the page PNGs: two portrait
   pages, red square bottom-left, a coloured arrow with "TOP" on the left side, text running bottom-to-top.
2. Open `job.pdf` in Preview: 2 pages, 153×243 pt. `command.txt` has the exact `lp` command.
3. Optional, from Terminal, to see what the queue accepts:
   `lpoptions -p <queue> -l | grep -E 'UTDuplex|CF|CB|PageSize'`

## Print 1: front only

Settings: Sides = Front only. Overcoat is always applied to both sides. Print.

Look at the card with the magnetic stripe side as you would normally hold it (long edge horizontal):

| What you see | Fix |
|---|---|
| Red square top-left, "TOP" arrow at the top edge, FRONT readable | Correct. Keep. |
| Upside down (red bottom-right, text upside down) | Tick **Rotate front 180°** (or advanced option `CFRotation=1Rotation`) |
| Turned 90° (text runs vertically) | Switch **Turn card to portrait page** to the other direction |
| Turned 90° and upside down | Other turn direction **and** Rotate 180° |
| Mirrored (text reversed) | Not fixable by a setting. Stop and report it: the page needs a mirror step. |
| One edge cut off or a white sliver | Try **Fit = Stretch**, and report which edge |

Re-print after each change (the Preview pages show what you are sending; the card is the ground truth).

## Print 2: back only

Sides = Back only. Same table. The back is often rotated 180° compared with the front because the printer flips the card:
if so tick **Rotate back 180°** (or `CBRotation=1Rotation`).

## Print 3: both sides in one duplex job

Sides = Both. Front and back must both be upright and correctly paired (front text on the front face).
If the back is correct only when printed alone but not in the duplex job, report it (odd/even page order is the suspect).

## Black check

On the card, the **pure black** text line and the bottom-left black square should be dense, matte resin black with sharp edges.
The **near black** line (#101010) is composite black (YMC): slightly warmer and softer. If both look composite, the PDF→raster step
altered pure black. Report it, and try **Black = Composite** or **Mono K** for black-only cards meanwhile.

## What to report back

Which table row applied for front and back, whether Stretch/Crop mattered, the black check result, the printer model as shown in
`lpstat -p`, and the queue's PPD option list (`lpoptions -p <queue> -l`).

## Colour check (optional, 1 card each)

The app sends the colour mode as the PPD option `ColourCorrection`. LUT modes and Gamma are left to the driver. Sliders are baked into the bitmap (the driver cannot do them). Mapping for a print from the app:

| CardDeck Colour Correct | What the app sends |
|---|---|
| None | `ColourCorrection=0ColourCorrection` |
| Gamma 1 / Gamma 2 | `ColourCorrection=1ColourCorrection` (Driver managed; bitmap not pre-corrected) |
| Perceptual | `ColourCorrection=2ColourCorrection` |
| Saturation | `ColourCorrection=3ColourCorrection` |
| Relative Colorimetric | `ColourCorrection=4ColourCorrection` |
| Absolute Colorimetric | `ColourCorrection=5ColourCorrection` |

1. Check the queue has the choices: `lpoptions -p <queue> -l | grep ColourCorrection` (expect `0ColourCorrection` to `5ColourCorrection`).
2. Print the same card with **None** and with **Absolute Colorimetric**. White should look slightly warm/off-white and
   greens clearly more saturated in the second; the app's preview of Absolute should look like that print.
3. Gamma 1/2 are not baked into the bitmap. Baking the on-screen curve and sending `ColourCorrection=None` printed light, washed-out colour next to Hub. The driver gets Driver managed for both, so a Gamma 1 print and a Gamma 2 print from CardDeck match each other and will not match the on-screen preview exactly. Sliders are baked (the driver has no slider options).
4. A dry-run command (with `-o ColourCorrection=...`) is in `command.txt` from `carddeck job` or `carddeck calibrate`.
