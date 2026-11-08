# Changelog

What changes between releases, for the people who use Chitrakar. The
build diary — every step, with its reasons — is `git log`; where things
stand and what is left before a release is `docs/PLAN.md` §0.

## Unreleased — towards 1.0

Nothing has been released yet. This is what the first release holds.

### Editing

- A non-destructive document: every layer stays live — shapes, paths,
  text, pictures, adjustments and filters — and every change is one
  undoable step with a labelled history.
- Vector: rectangles, ellipses, polygons and stars, a library of shapes,
  lines, pen and brush paths with bezier handles, a node tool,
  booleans, offset and simplify, a knife, blends between shapes, live
  copies, repeats. A border sits inside, across or outside an outline,
  on paths as on shapes.
- Text: shaped by its font, any face loaded, bold, italic, underline,
  strike-through, styled a word at a time, wrapped and aligned, set
  along a path, typed on the canvas, found and replaced.
- Pictures: placed, cropped, straightened, warped by their corners,
  painted on with live strokes, an eraser, a clone brush that heals,
  dodge and burn.
- Adjustments and filters over everything below or on one layer:
  exposure, brightness and contrast, levels, curves over a histogram,
  hue and saturation by band of colour, shadows and highlights, colour
  balance, black and white, gradient map, colour lookup, blurs,
  sharpen, noise, vignette and more.
- Masks — shapes, brushed, gradients (graduated and radial filters) —
  clipping, groups, artboards, sixteen blend modes, live effects
  (shadows, outlines, bevels, overlays), named styles and a palette of
  named colours that the document follows.
- Selections: marquee, ellipse, lasso, wand, subject, feathered,
  combined, kept by name.
- Snapping, smart guides, rulers in pixels, millimetres or inches, a
  grid, measuring, layer comps.

### Photographs

- Camera raw files from every maker rawler reads (CR2, CR3, NEF, ARW,
  RAF with X-Trans, DNG, RW2 and more), developed in a window of their
  own: white balance in kelvin, exposure, highlights and shadows, tone
  curves, a colour mixer, colour grading, clarity and dehaze, detail
  and noise reduction, lens corrections from the file or from lensfun,
  chromatic aberration and defringe — and developed again later, with
  one undo.
- A shoot at a time: develop and export many raw files in one go, with
  presets and a watermark.

### Colour

- Real colour management: ICC profiles on import, CMYK documents with
  press profiles, soft proofing, ink kept as ink in TIFF and PDF.

### Files

- `.chitra` documents (a ZIP of the document, its pictures and its
  profile), with crash-recovery drafts kept per tab.
- Opens and places PNG, JPEG, WebP, GIF, BMP, HEIC and AVIF (where the
  system decodes them), TIFF (bits, CMYK, ICC), PSD with layers, SVG,
  PDF and PDF-compatible Illustrator files, and raw files.
- Exports PNG, JPEG (with the camera's EXIF, never GPS), WebP, SVG,
  CMYK TIFF, PDF (live vectors and text, bleed and crop marks), and a
  layered PSD — the whole page, what is picked, frames, or every kept
  region; sized at @1x/@2x/@3x or to a long edge.

### Everywhere

- Desktop (Windows, macOS, Linux) through Tauri, with files by path,
  recent files and the system's menus; installable in a browser and
  working offline; phone and tablet layouts with touch, pen pressure
  and palm rejection.
- Keyboard: every command rebindable, menus and windows usable from the
  keyboard alone, shapes and text added from the menus.
- Light and dark themes, an accent colour of your own, interface size.
