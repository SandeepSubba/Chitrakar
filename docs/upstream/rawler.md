# rawler: issues to report upstream

Drafts for <https://github.com/dnglab/dnglab/issues> (rawler lives in
that repository). Nothing here has been posted yet. Each one is worked
around in `core/rawin/src/lib.rs`, so the editor stays correct whether
or not rawler changes. If a fix lands upstream, take the workaround out
in the same commit that bumps rawler.

All found with rawler 0.8.0 on samples from raw.pixls.us, developed by
this editor and compared against the camera's embedded JPEG.

---

## 1. Olympus E-10: CFA pattern is RGGB, not BGGR

**Where:** `data/cameras/olympus/e-10.toml`, `color_pattern = "BGGR"`.

**What happens:** Developed with the table's pattern, the picture has red
and blue swapped: foliage comes out cyan and skin comes out blue.

**Evidence:** With `RGGB`, the development matches the embedded preview.
dcraw and LibRaw also treat the E-10 as RGGB.

**Suggested fix:** `color_pattern = "RGGB"`.

**Our workaround:** `correct()` replaces the CFA for
`("Olympus", "E-10")`.

---

## 2. Kodak EasyShare Z981: CFA pattern looks like GRBG, not BGGR

**Where:** `data/cameras/kodak/z981.toml`, `color_pattern = "BGGR"`.

**What happens:** As read (with the `easyshare_offset_hack` hint), the two
sites the table calls green do not average the same, but the other
diagonal's two do. So the greens are on the other diagonal from the
table's BGGR. Of the patterns that fit, only GRBG makes a white subject
(white roses) come out white.

**Caveat:** This is from one sample. The Z990 next to it in the table is
BGGR, as rawler says. A second Z981 file would settle it.

**Suggested fix:** `color_pattern = "GRBG"`, after checking against
another file.

**Our workaround:** `correct()` replaces the CFA for
`("Kodak", "EasyShare Z981")`.

---

## 3. Kodak KDC: as-shot white balance read from the wrong table

**Where:** `src/decoders/kdc.rs`, `KdcDecoder::get_wb`. When the
makernote has no `KdcWB`, it reads bytes 148/150 of `KodakWB`.

**What happens:** The result has nothing to do with the light the photo
was taken under. Across Kodak samples it ranged from a blue cast to a
red cast up to sixty times too strong (for example, the
`KODAK P712 ZOOM DIGITAL CAMERA` sample).

**What the file says:** Kodak's own IFD (tag `0xFE00`) holds a table of
preset balances: tags `0xFA25`, `0xFA28`, `0xFA27`, `0xFA29`, two unused
slots, then `0xFA2A`. Tag `0xFA0D` is the index of the preset the camera
used. This is dcraw's `wbtag` lookup.

**Suggested fix:** Follow `0xFA0D` into the preset table, and fall back to
the current read only when either tag is missing.

**Our workaround:** `kodak_balance()` parses the IFD itself, and the
`a_kodaks_balance_is_the_preset_it_says_it_used` test covers it.

---

## 4. Canon sRAW / mRAW: black level subtracted twice

**Where:** `src/decoders/cr2.rs`. The YCbCr → RGB conversion for
`sRaw1`/`sRaw2` already removes black (the darkest output sites are 0).
After that, `get_blacklevel` still attaches the full-size raw's black
level (2047 on a 7D, 1023 on a 40D).

**What happens:** Anything downstream that subtracts `blacklevel`
(rawler's own `RawDevelop` included) takes black off a second time. That
loses most of the blue channel and gives a yellow-green cast. Seen on the
40D and on the 7D in both mRAW and sRAW.

**Suggested fix:** Give a zero black level for `cpp == 3` sRaw images, or
skip the subtraction during the conversion.

**Our workaround:** `correct()` zeroes the black level when `cpp == 3`,
the make is Canon and `camera.mode` starts with `sRaw`.

---

## 5. Fujifilm RAF: orientation is always `Normal`

**Where:** `src/decoders/raf.rs` never sets `RawImage::orientation`.

**What happens:** Photos taken with the camera held upright open sideways.
`raw_metadata()` reads the EXIF orientation correctly (6 or 8 for upright
shots); it just never reaches the decoded image.

**Suggested fix:** Set `orientation` from the EXIF in the RAF decoder, as
the other TIFF-based decoders do.

**Our workaround:** When the decoder reports `Normal` or `Unknown`, we ask
`raw_metadata()` for the EXIF orientation.

---

## 6. Samsung Galaxy S23 linear DNG: values read far past white

**What happens:** The S23's linear (already demosaiced, three samples per
pixel) DNG decodes with red at or past full scale everywhere, and green
and blue at almost nothing, so it develops to a flat red field. More than
1% of samples are over 1.1× the white level, which a correct read never
gives.

**Likely cause:** The strip/tile layout or bit packing of this
LinearRaw DNG is misread (sample order or byte order), rather than a
colour-table problem.

**Our workaround:** `misread()` refuses any file where more than 1% of
samples exceed 1.1× white, so the user gets a clear error instead of a
red picture. This is a guard, not a fix: those files still don't open.

---

## 7. Two colour matrices: no defined choice, and no blend

**Where:** `RawImage::color_matrix` is a `HashMap<Illuminant, _>`.

**What happens:** For a DNG or table entry with two matrices (typically
standard light A and D65), "the first one" depends on the hash map's
iteration order, which changes from run to run. A Xiaomi DNG swapped
between its daylight and tungsten matrices across runs, so the same file
developed with two different colourings.

**Suggested fix:** Use an ordered map, or a struct with named slots. The
DNG specification also says how to choose: interpolate between the two
matrices by inverse colour temperature of the as-shot white point
(Chapter 6, "Mapping Camera Color Space to CIE XYZ Space"). That would be
worth offering as a helper.

**Our workaround:** `as_shot_matrix()` does the inverse-temperature blend
(McCamy's approximation for the white point's CCT). With only one
matrix, it picks the one measured nearest D65, with ties broken by
illuminant number, so the choice is deterministic.

---

## 8. Feature request: expose Fujifilm's lens corrections

RAF files carry the camera's own distortion (`0xF00B`), lateral chromatic
aberration (`0xF00F`) and vignetting (`0xF010`) tables in the RAF
directory. They come in a 19/29/19-value layout on older bodies and
23/31/23 on newer ones. When the body shoots in a crop mode (makernote
`0x104D` = 2 or 4), the radii are scaled by 1.25. rawler doesn't surface
these. We parse them ourselves in `fuji_lens()`, and checked the result
against the camera's own JPEG: corrected scale matched to 1.016 ± 0.003
across all radii.

---

## A note on panics

Some decoders turn malformed input into panics and rely on
`catch_unwind`. A WebAssembly build can't unwind, so a bad file takes
down the whole module. We run rawler in a worker and throw the worker
away after any failure. Returning `Err` from those code paths would make
rawler usable in-process on wasm.
