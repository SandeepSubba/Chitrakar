# Chitrakar — Architecture & Roadmap

Chitrakar ("painter") is a modern, multiplatform photo + vector editing app built around
two non-negotiable principles:

1. **Non-destructive everything** — the document is a tree of live objects (shapes,
   images, adjustments, filters, masks). Pixels are only ever *rendered*, never baked.
   Any edit can be revisited or removed at any time.
2. **Real color management** — documents can be RGB or CMYK, with ICC-profile-correct
   import, display (soft proofing), and export. This is designed into the pixel
   pipeline from day one, not bolted on.

**Target platforms:** Windows, macOS, Linux, iPadOS, iOS, Android.

---

## 0. Where things stand (read this first)

*Handoff block — keep it current; it exists so a fresh session can resume
without reading anything else.*

- **Branch:** `claude/multiplatform-photo-vector-editor-enghs5`.
- **Working today:** a real editor. Draw rects (square or round-cornered),
  ellipses, regular polygons and stars of three sides to a couple of
  dozen, straight
  lines and pen paths (straight
  or smooth), brush freehand strokes that land as editable paths and swell
  with pressure or slow strokes, place images, add live text; move/scale with handles and live
  drag preview — a dragged corner keeps the shape's proportions and
  shift lets go of them, which is the way round a picture wants: letting
  go of a photograph a little squashed is a mistake nobody notices until
  it is printed; a corner caught on a snap line lands exactly on it and
  the other side follows from the shape. Shift while a shape is being
  drawn squares off the box being dragged — a circle rather than an
  ellipse, a square page rather than a wide one — and shift-clicking
  with the pen holds the segment to an eighth of a turn. Shift means the
  other one either way round: a shape being drawn has no proportions
  yet, so shift is what asks for the one worth naming; a shape being
  resized has them, so shift is what lets go. Alt says the drag is about
  a middle rather than a corner — a shape drawn out from where the drag
  began, a shape resized about its own middle — which is how a circle is
  put on a target rather than beside one, and the two modifiers combine — turn with a rotation knob, flip a selection about its
  own box; adjustment layers (exposure, brightness/contrast, hue/sat,
  white balance, vibrance, levels,
  curves drawn on a graph in the panel — a master curve and one per
  channel run after it, which is what a colour grade is made of, with
  the channels not in hand drawn faintly behind the one that is; the
  curves and levels graphs are drawn over a histogram of what the layer
  actually sees — everything composited under it, not the finished page
  — so a black point can be set where the picture stops rather than by
  eye, and levels' own points are shown in the encoding that histogram
  is drawn in (they are kept in linear light, where the adjustment
  works) so that what a slider says is where the graph says the picture
  is; white balance can be taken from the picture rather than typed —
  point at something meant to be grey and the temperature and tint that
  neutralize it follow, worked out from what that layer is given rather
  than from the finished page, which already carries whatever balance is
  being replaced (the gains are a channel each, so making a colour
  neutral is two equations with an exact answer, clamped to what the
  sliders can say);
  Auto sets the two input points from that same histogram, a
  thousandth of the picture left outside at each end so that a speck of
  dust or a clipped highlight is not what decides where a picture's
  black is;
  shadows and highlights, which moves the two ends of the tone range and
  leaves the middle where it is — the first thing asked of a photograph
  after exposure, since a face against a window is dark because the
  window is bright and no single exposure fixes both; each end's pull
  falls off as the cube of the distance from it, what moves is the
  pixel's brightness and its colour comes along, and both run the other
  way for deepening a shadow rather than lifting one (a function of the
  pixel alone: Photoshop's reads the neighbourhood too, which is where
  its local contrast and its halos come from);
  black and white, which is a recipe rather than a switch — the weights
  decide which colours come out light, so a high red weight darkens a
  blue sky the way a red filter on the lens did, and they are normalized
  by their own total so a slider changes the mix and not the brightness;
  gradient map, where every tone is replaced by the colour at its own
  place along a ramp — duotones, split tones and the whole family of
  graded looks — read at the brightness a device shows, which is what
  lands the middle of the ramp on the tones that look middling; and
  invert, taken on the shown values too, since light inverted is not
  what anyone means by a negative) and
  filter layers (gaussian blur, sharpen, pixelate — squares of one
  colour each, the average of what they covered, which is what a face or
  a number is taken out of a picture with — and noise, which is grain)
  — over everything below, or
  scoped to one layer, which groups the two so the group's isolation does
  the confining — masks on any layer — an
  inscribed ellipse or rectangle, any shape handed down to the layer
  below, or one brushed on by hand — dragged and resized on the canvas, groups,
  reorder (by the arrows, or by dragging a row above, below or into
  another), the sixteen blend modes the W3C compositing spec names —
  grouped in the picker the way editors group them, and read over the
  values a device shows rather than over linear light, which is what
  makes a page look the same in the engine as in the SVG and PDF it
  exports, duplicate/copy/cut/paste (subtree and pixels included, across
  documents), copy and paste a layer's look on its own — what it is
  painted with, what hangs off it and how it sits on what is under it,
  but never its shape — onto any number of layers in one entry,
  delete, align and distribute a multi-selection, combine
  shapes with union/subtract/intersect/exclude into a compound path that
  can carry holes, opacity/blend,
  rename, lock (a locked layer is drawn but neither picked nor moved from
  the canvas), and a layer carried to the front or the back of its own
  group in one step (Ctrl+Shift+] and Ctrl+Shift+[) rather than one step
  per layer in the way, labelled history with jump-to-state. Opacity and blend
  reach every picked layer at once, in one entry.
  Transforms nest: a group moves, scales and turns as a unit, and
  dissolving one folds its transform into its children. Documents are any
  size, chosen from presets or typed, in RGB or CMYK, and the crop tool
  re-frames one after the fact — the page becomes the dragged rectangle
  and the picture stays where it was inside it. A crop can be held to a
  ratio — a print's, a screen's, a square, or the page's own — which is
  what a photograph is nearly always cropped to rather than to whatever
  the drag happened to be; the frame drawn while dragging and the crop
  taken on the way up are the same box worked out by the same function,
  clamped to the page and then fitted to the ratio inside what is left,
  so a square crop that runs off the bottom comes out square and smaller
  rather than not square at all. The thirds are drawn over the frame,
  which is where a horizon goes. Canvas size is the
  other half of that: cropping can only ever take room away, and this
  gives the page room — the couple of centimetres of white around a
  photograph that every print asks for — with one of the page's nine
  points staying where it is while the rest grows or shrinks around it,
  so nothing has to be moved by hand. What falls outside a page made
  smaller is off the page, not gone. The Page menu also turns the
  page a quarter of the way round either way, or the whole way over: an
  odd turn stands it on its end, and layers and guides go round with it,
  so a portrait photograph opened into a landscape page is one menu item
  away from being the right way up; and it mirrors the page across
  either middle, which is a different thing from flipping a selection —
  the guides cross with the artwork. And it straightens: any angle at
  all, turned about the page's own middle and cropped back to the shape
  the page was, which is what a crooked horizon wants. The turn shows as
  the slider is dragged rather than only once it is taken, so a horizon
  is laid level against the edge of the page by eye; the whole gesture
  is one entry, and cancelling leaves nothing behind. Nothing is
  resampled — a turned layer is a transform, and the corners the turn
  brings in are off the page rather than gone — which is a thing a pixel
  editor cannot say about straightening. Turning, mirroring and
  resizing are one transform of the page's own space put through one
  function, so what travels with the page is decided once: the layers,
  what masks them (a mask is written in the space its owner is placed
  in, so it does not travel with the layer's own transform and has to be
  taken along — left behind it goes on hiding the part of the page it
  used to cover, which for a page that moved out from under it is the
  whole layer), the offsets of what they cast, and the guides.
  Edges are anti-aliased: rect fills analytically, path fills by a scanline
  rasterizer (exact horizontally), the rest by coverage sampling, and vector
  mask edges feather the same way. Placed images sample bilinearly in
  premultiplied linear space, with the image outline antialiased too.
  A stroke can be broken up: a pattern of lengths on and off, walked by
  arc length along the outline so a dash crosses a corner the way it
  would along a wire rather than restarting at every anchor, with the
  usual patterns in the panel and the same lengths carried into SVG and
  PDF. A dashed outline is still picked along the whole of it, so
  clicking a gap catches the line.
  A line can point at something: an arrowhead, a tick across the end or
  a dot, asked separately of its two ends, on an open path — a ring, a
  rect and an ellipse have no ends to put anything on. What one carries
  is sized from the line's own width rather than given a size of its
  own, so making the line thicker keeps the head in proportion, and it
  goes where the line stops rather than where each dash does, so a
  dashed arrow has one head and not a dozen. They are stated as pieces
  of the stroke's own region, which is why they are painted in the
  line's colour, picked with the line, and drawn by the GPU without it
  being told about markers at all. SVG gets real `<marker>` elements in
  units of the stroke width, so a reader can still take one off; PDF has
  no markers and gets the outlines filled, which the exporter asks the
  engine for rather than working out a second time.
  A line says where it stops and how it turns: flat on the last point,
  rounded or squared off past it, and a corner carried out to where the
  two outer edges cross (up to four half-widths, past which it is cut
  off instead), rounded, or bevelled — asked of a path, which has ends
  and corners, and not of a rect or an ellipse, whose stroke is a band
  lying inside a closed outline. Every dash gets the same ends, which is
  what makes a dashed rule end square rather than round. The region a
  stroke covers is stated once, as a union of convex pieces
  (`chitrakar_render::stroke_pieces`), and both renderers read that one
  statement — the CPU tests a sample against the pieces, the GPU lays
  them down as geometry — so neither can invent a corner of its own.
  It travels: SVG's stroke-linecap and stroke-linejoin, PDF's J and j,
  and a placed SVG comes in ending and turning the way its file says
  rather than the way this engine defaults. A rect or an ellipse wears its stroke on
  whichever side of its edge is asked for — inside, so a thick border
  never grows the shape; across it, as SVG and PDF do by default; or
  outside, so a border never eats into the fill. Its outline has a
  distance of its own, so all three are exact, and the dashes that break
  any of them up walk the middle of the band they belong to. A path is
  stroked down the middle of its line whatever is asked, which is what a
  line means and all an open one could mean: putting a band to one side
  of a path wants its outline offset, and an offset outline is a guess
  where a distance is not. Nothing asked for is a band inside the edge,
  which is what a file written before there was an ask still gets.
  Neither SVG nor PDF can say which side a stroke lies on, so export
  writes both as a fill at full size and a stroke on the same shape
  moved half a width that way — which lands the band exactly where the
  engine draws it. Clipping to one side would say it too, but a clip's
  own edge is antialiased against an edge the fill already antialiased
  and the two do not add up: ghostscript drew a seam all the way round
  an ellipse until the clip became a moved shape.
  Fills can be linear or radial gradients with any number of stops,
  authored in the shape's own box so they follow it, aimed by dragging their
  ends and stops on the canvas, and exported as live SVG gradients. The
  stops mix on the values a device shows, not in linear light, because
  that is where SVG, PDF and every browser mix a gradient — red to blue
  through linear light passes a magenta a good deal brighter than the one
  an editor draws, so a ramp mixed there would change on the way out of
  the door. The CPU and GPU renderers walk the one ramp
  (`chitrakar_render::ramp_color`), so they cannot drift. Paths carry bezier
  handles you drag on canvas — alt-drag breaks a pair to make a corner —
  and converting from straight or smooth preserves the shape; they export
  as real cubic segments. Anchors go on and come off after the fact:
  double-clicking a path's outline puts one where it was clicked,
  splitting that segment so the curve stays exactly where it was, and
  alt-clicking an anchor takes it off.
  Any layer can carry live effects — drop shadow, outline and inner
  shadow, stacked in any combination, all cast from the layer's
  silhouette so they follow every edit.
  The paint brush (N) lays pixels on a layer of its own: a stroke is the
  line it was drawn along, the radius at every point of it (from a pen's
  pressure, or from how fast a mouse moved), the colour, and how far in
  from the rim its edge fades — kept as strokes, not as pixels, so any
  one of them can come back off. Its eraser rubs out this layer's own
  paint and leaves what is under it alone; a layer is picked where it
  has paint, so the empty part of one lets through what is beneath; and
  however many points a stroke gathered, it is one entry in history. A
  ring under the pointer says how big the brush is and how much of that
  is its fade, `[` and `]` resize it, alt-click takes the colour under it
  without putting it down, and shift-click runs a straight line on from
  where the last stroke ended.
  The clone brush (S) paints with what is already on the page: alt-click
  says where to read from, and every stroke carries that offset, so what
  it lays down is whatever the page shows there *now* — retouch the
  source and the clone follows, which is what a stamped copy of pixels
  could not do. With healing on (its default) it lays the source's
  texture down in the colour of the place it lands, so a patch lifted
  from somewhere lighter sits into its surroundings instead of showing
  as a disc. The page is snapshotted before a stroke is laid, so a
  stroke crossing its own source lifts what was there when it began
  rather than what it has just put down.
  Artboards (F) are pages within the page: a frame dragged out anywhere
  on the canvas, with a ground of its own, that cuts whatever goes into
  it to its box. A shape drawn inside one goes into it, in its own
  coordinates; a layer dragged onto its row in the panel goes in without
  moving on the page (changing parents no longer shifts a layer — the
  new parent's space is taken back out of its transform, which fixes
  dropping into an off-origin group too); and File › Export every
  artboard writes one PNG per frame, at the frame's own size, named
  after it, with nothing of the page around it; File › Export PDF of the
  frames writes them as the pages of one file instead, in the order they
  sit on the document, each page its frame's own size and live where PDF
  has the words — a brochure laid out as artboards comes out a brochure.
  Both are offered only when there are frames to make pages of.
  Frames carry into SVG as
  a clipped group and into PDF as a rectangle clip, so both stay live
  vectors. A frame is resized rather than scaled: dragging a corner
  changes how many pixels it is and leaves what is in it the size it
  was (pulling the west or north edge carries the contents with it,
  since they are written against the frame's own corner), and the W/H
  fields set that same number, and the panel offers the sizes anyone
  actually asks for — screens, posts, and paper worked out through the
  document's own resolution. What is inside it moves by how each layer
  is pinned — left, centre, right or both sides, and the same down the
  page — so a frame taken from one screen size to another lays itself
  out rather than needing every layer dragged. Its ground is a colour in
  the panel, or none at all, which makes the frame a window onto the
  page.
  Any layer can be given live copies of itself — a copy draws whatever
  that layer holds, wherever the copy is put, so changing the original
  changes every copy of it at once, which is what a component is. The
  original's own placement is not part of what travels, so moving the
  original moves only the original; a copy carries its own transform,
  opacity, blend and mask on top. The panel says what a copy follows and
  takes you there. A copy that could reach itself would have nothing to
  draw, so the document refuses to make one and stays as it was. A copy can differ where it has to: given one of the
  original's layers as its own, it keeps that one and follows the
  original in everything else — a label with a different string, a panel
  in a different colour. The panel lists what the original holds, with a
  way to take one and a way to give it back. Only a plain group's layers
  can be stood in for; an original that is drawn as a whole for its own
  opacity, blend, mask or effects says so rather than quietly ignoring
  the ask. Copies
  stay live in both vector exports: the original's markup again, inside
  the copy's place.
  A layer can be confined to the one below it — Ctrl+Alt+G, or the hook
  in the layer bar — so it shows only where that one does and goes when
  it goes: a texture cut to a shape, a photo poured into lettering, or,
  most of all, an adjustment or a filter that reaches one layer instead
  of the whole page. A run of them stacks against the same layer.
  Rubbing at a layer that is not a paint layer takes a piece out of it
  instead: the stroke goes into a painted mask, so the layer is
  untouched and the brush puts the piece back — which is how part of a
  photo is taken out without touching the photo.
  Groups and frames fold shut in the layer panel, so a document of
  several artboards reads as a list of artboards rather than of
  everything inside them. A picked frame exports on its own from the
  File menu, beside the one that writes every frame at once.
  Layer rows carry a small picture of the layer itself — what the page
  would draw of it, effects and all, fitted into its own square — in
  place of the glyph that says what kind it is, and a picture of its
  mask beside that, fitted the same way so the two line up: white where
  the layer shows through, clear where it is hidden. Both are
  regenerated a breath after the document settles rather than every
  frame.
  Dragging a layer snaps its edges and centre to the page's and to the
  other layers', showing a guide on the line it caught; ctrl/cmd drags
  free of it — and the same lines catch a resize handle, the corner a
  shape is being drawn out to, and a pen's anchor as it is put down. A
  shape has no box to align yet, so what catches is the corner under the
  cursor, at both ends of the drag: a rect laid against the page's edge
  starts on it. Shift wins over the lines outright — it asks for an
  exact shape, and a square nudged onto a line would be neither square
  nor on it — so a guide never says a corner is somewhere it is not. The
  handles belong to the move tool alone: with anything else up the
  pointer is there to draw, paint or crop, and a handle over the corner
  a rect was about to start at would resize what is picked instead of
  drawing. A
  multi-selection — dragged out as a band over empty canvas, or built
  ctrl-click by ctrl-click — moves as one (alt-dragging takes a copy and
  leaves the original), by drag or by
  arrow key (shift for a coarse step) — in a single history entry. Exact
  placement is typed: the panel carries X/Y/W/H in document pixels, and
  the angle beside them — the knob turns a layer by eye, and some things
  have to be at forty-five degrees exactly. Typing one turns the layer
  about the middle of its own box, which is where the knob turns it, so
  the number says where it stands rather than moving it.
  The document keeps a palette of its own: colours added from whatever
  is being drawn with, saved with the file, clicked to draw with and to
  give to the picked shape or block of text, alt-clicked to take out
  again. In a CMYK document they are ink, and resolve through the press
  profile exactly as a fill does.
  A right-click on the canvas offers what can be done with what is under
  the pointer, where the pointer is: cut, copy, duplicate, the front and
  the back, group and ungroup, delete — and on bare canvas the things
  that need no layer at all. Right-clicking something not already picked
  picks it first, since a menu about "this" has to be about what was
  pointed at, and a menu asked for against a corner is kept inside the
  window rather than hanging off it.
  The eyedropper (I) takes the colour the page shows under the cursor —
  the composite, effects and opacity included — as the colour to draw
  with, and gives it to the picked shape or block of text.
  An SVG placed, dropped or pasted comes in as a group of editable shape
  layers — paths with their curves, solid and gradient fills, strokes,
  group opacity, text as outlines — in document space, one undo step
  (raster images inside an SVG are left out; a nonzero fill rule reads
  as even-odd).
  Documents carry a resolution (presets and the New dialog set it, with
  the page's size on paper shown), and View › Pixels/Millimetres/Inches
  reads the rulers, the geometry fields and the status line in that
  unit through it. Rulers run along the canvas edges; dragging out of one places a guide
  that layers snap to and that saves with the document, and dropping it
  back throws it away.
  The view has the keys every editor has: Ctrl+= and Ctrl+- zoom about
  the middle of the window, Ctrl+0 fits the page to it and Ctrl+1 shows
  the page's own pixels one for one.
  A window too narrow to hold a column of layers beside the canvas — a
  phone, a tablet held upright, a window dragged small — lays the panel
  over the canvas instead, out of the way until the bar's own button asks
  for it, and the bar drops what only tells you something (the name over
  the door, the size of the page) so that nothing hangs off the side and
  slides the whole page about.
  A tablet has neither wheel nor space
  bar, so two fingers are the view's there: a pinch zooms about the point
  they began around and their middle carries the page, one gesture doing
  both because that is one gesture to a hand. One finger stays the tool's,
  as a mouse is, and the second takes over — whatever the first had begun
  is let go of rather than left half-drawn, since half a rect dragged out
  on the way to a pinch is not something anyone meant to draw.
  Zooming in re-renders rather than magnifies: the engine composites a
  viewport at the resolution the canvas is displayed at, so outlines,
  gradients and glyphs are re-solved instead of interpolated, and a page
  costs a screenful of pixels to show however big it is.
  Text is shaped by the font (rustybuzz: ligatures, kerning, combining
  marks; complex scripts once a font for them is registered), can be set
  in DejaVu Sans, Sans Bold, Serif or Sans Mono — the bundled face is in
  the wasm, the rest are fetched from `app/public/fonts` at startup and
  registered with the engine, so adding a face is dropping a file there;
  File › Load font… registers any TrueType/OpenType file for the page's
  lifetime; Bold and Italic ask for a weight and a lean rather than
  naming a face — the family's "… Bold", "… Italic"/"… Oblique" or
  "… Bold Italic" cut is used when one is registered (Sans Bold and Sans
  Mono Oblique are shipped), and whatever the family cannot answer for
  the rasterizer supplies itself: a lean by shearing the outlines, a
  weight by laying each one down again beside itself until the stems
  have thickened. Asked for bold italic when only the italic cut exists,
  the italic is set and the weight put on over it. The thickening is
  calibrated against a real cut — a line of DejaVu Sans thickened this
  way sets to the length DejaVu Sans Bold sets it to — and travels as
  SVG's font-weight and, on a page, as glyphs filled and then stroked,
  which is a page's own way of putting weight on an upright;
  underline and strike-through toggles draw their bands per line (and
  travel into SVG as text-decoration and PDF as rectangles). All four,
  and the colour, apply to a selection when there is one: select a word
  in either text box and Bold, Italic, Underline, Strike or the colour
  well styles that word rather than the block, as a style run. Runs are
  byte ranges over the text saying only what they change, so a block is
  cut into pieces where they start and stop and each piece is shaped and
  drawn in its own cut of the face — shaping stops at a boundary, which
  is right, since a font has nothing to say about how its letters sit
  against another's. A run's size is the block's: mixing sizes in one
  block would change where the lines sit, and is a separate thing. They
  follow the text they were put on through an edit, they travel as SVG
  tspans saying only what they change and as one PDF text object a
  stretch — still selectable, still searchable — and a run that would
  only repeat what the block already says is dropped rather than kept as
  a run that silently overrides it later. a
  double-click on a block types into it on the canvas (the engine draws
  the letters under a transparent textarea placed by the block's own
  matrix; Escape cancels, Ctrl+Enter or a click away keeps, one history
  entry either way), a block can be set along any shape layer's outline
  (the panel's Along picker copies the outline into the block, each
  glyph turns to follow it, an offset slides it along, open guides drop
  what runs off the end and closed ones wrap; SVG gets a textPath and
  the PDF turned text matrices), and a saved
  `.chitra` carries the faces its text is set in (all but the bundled one)
  under `fonts/`, registering them on open, so a document reads the same
  wherever it is opened next —
  rasterizes at
  the size it is actually seen at, and carries alignment, line spacing,
  tracking and a wrap width for paragraph text.
  A draft of the open document is written to IndexedDB a breath after
  every change, and a fresh visit offers it back (Restore or Discard),
  so a closed tab or a crash loses nothing. The document has a name —
  typed in the bar, taken from the file it was opened from, restored
  with the draft — and every save and export is named after it.
  The shapes share one slot in the rail — the one last used sits in it
  and the rest are a click away, which keeps a rail of a dozen tools
  from becoming a rail of twenty; the slot follows the keyboard too, so
  a shape taken up by its letter is the shape the rail shows. A polygon
  and a star are inscribed in the box they are dragged out of — a point
  at the top, the rest around — and come out as paths, so every anchor
  is draggable the moment it is drawn and every exporter already knows
  what they are; the number of sides is typed beside the slot. A line is
  the drag itself, end to end, stroked rather than filled, since an open
  line has no inside — and it is the one shape whose box may be nothing
  on one side, so it is held to a length rather than to two sides.
  The rail can be carried off the edge by its grip and put down anywhere
  over the canvas (double-clicking the grip, or dropping it back at the
  left, docks it again), and the layer panel is as wide as its edge is
  dragged to be. Both are remembered between visits, so a workspace
  arranged once stays arranged.
  Color: embedded ICC honored on import, CMYK documents with press profiles,
  soft proofing + gamut warning. Files: `.chitra` save/open; export PNG (at 1x, 2x or 3x, or of just
  the selection — rendered at that size, not upsampled), JPEG, SVG,
  CMYK TIFF, PDF. Desktop app packages (deb verified locally; CI builds
  Win/macOS/Linux installers on a `v*` tag).
- **Renderer performance:** the transform inverse is solved once per shape
  (`Inverse`), not per coverage sample — a boundary pixel asks for up to
  twenty-one. Groups only paint where their contents can land, and a group
  that nothing inside reads the backdrop through (opacity 1, Normal, no
  mask, no blended/adjustment/filter descendant) skips its isolation
  surface entirely. Together those took a full A4/300dpi render from
  ~370 ms to ~240 ms (fat LTO in the release profile later took another tenth
  off every figure and 6% off the wasm bundle), and one plain folder in an A4 document from ~250 ms
  of overhead to none. A group that *does* have to be isolated — opacity,
  a blend, a mask, or something inside reading the backdrop — is now
  isolated on a surface the size of the box it can land in rather than
  the size of the page, so at A4 a group holding one small shape went
  from 145 ms to 84 ms, which is what the same group costs when it needs
  no isolation at all; the same window makes a layer's effects cost what
  the layer alone costs. What remains at that size is mostly memory
  bandwidth: the surface is 16 bytes a pixel (139 MB at A4), and each
  full-canvas pass over it costs ~55 ms. `cargo test --release -p
  chitrakar-render -- --ignored --nocapture --test-threads=1` runs the
  timing probes. A layer with live effects is windowed the same way —
  its own surface and every field built from it cover only where the
  layer can land — which took one small shape with a drop shadow at A4
  from ~220 ms to ~86 ms, against a ~82 ms baseline. Nesting the two
  windows is where a mistake would show, so a test puts a shadowed
  layer inside an isolated group and compares it, pixel for pixel,
  against the same layer drawn on its own. Interactive rendering goes
  through `Session::set_viewport`, which composites only what the canvas
  can see: an A4 page at 300dpi shown on screen is 15 ms rather than
  169 ms, and the zoom is no longer capped by what a full-page surface
  would cost. Blending reads the values a
  device shows, which means crossing the transfer curve nine times a
  pixel; tabulating that curve (4096 steps, straight lines between, and
  a test holding it to a part in ten thousand of the real one) took a
  blended A4 page at 300 dpi from 1.87 s to 1.26 s, against 0.47 s for
  the same page composited plainly. A blend does more work than
  source-over and always will; the interactive path renders a screenful
  rather than a page, so what is felt is a fraction of that.
  A filter is a function of the document, not of the window it is seen
  through: a pixelate grid is laid out on the page, so panning slides
  the picture under a grid that stays put on it, and grain is settled by
  where a speck sits on the page and by the layer's seed and by nothing
  else, so the same page grains the same way however it is drawn. That
  second part is what lets a region be redrawn on its own without a
  seam, and a block's own width is part of `filter_reach` so a block
  hanging over the edge of a redrawn region is still averaged whole.
  An adjustment works out what it can before the pass rather than per
  pixel (`chitrakar_render::prepare`): a curves adjustment's tables, and
  a gradient map's ramp — resolved and sorted once instead of eight
  million times, which took a full-page map from 1.48 s to 0.95 s.
  Resolving it there is also where a CMYK document's stops meet its
  press profile, which is the last place the document is to hand.
  A placed photo shown smaller than
  its own resolution is box-filtered over the texels each device pixel
  really covers (up to four taps an axis), so shrinking one settles
  instead of crawling.
- **Opening a file that is not right:** a `.chitra` that is truncated,
  bent, or not a zip at all is refused rather than survived — a test cuts
  a good file short at every tenth of its length and flips a byte every
  seventh through its first kilobyte. A page is held to what the engine
  could actually draw (a hundred million pixels, thirty thousand a side,
  at sixteen bytes a pixel): a file claiming more is refused on the way
  in, where it can be said, and `Session::new` gives back the largest
  page there is rather than one nothing could render.
- **Export fidelity has a witness:** `resvg_draws_the_same_page_the_engine_does`
  exports a page of one-of-everything to SVG, has resvg draw it, and holds
  the result against the engine's own render — mean channel difference, a
  count of badly-different pixels, spot checks on every element, and the
  box the text's ink sits in. It found the stroke bug it now guards, and
  the PDF exporter has the same witness in ghostscript. What the two
  cannot agree on is partial alpha: the engine composites in linear light
  and every SVG consumer composites in the encoding a device shows, so a
  half-opaque red over paper is 255,188,188 here and 255,128,128 there.
  Gradients and blend modes were moved to the shown encoding because each
  is a self-contained mix; moving *all* compositing there would change
  every antialiased edge and every resampled image, and cost a transfer
  crossing per channel per pixel on the hot path. It is a real divergence,
  left open deliberately.
- **Verify before committing:** `cargo test --workspace` (~298),
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all`,
  and in `app/`: `npm run build && npm run test:e2e` (~685 browser
  assertions). Both suites self-skip CMYK-profile steps unless
  `CHITRAKAR_TEST_CMYK_ICC` points at a CMYK .icc. The toolchain is pinned
  in `rust-toolchain.toml` and CI installs from it, so the clippy that runs
  locally is the clippy that runs in CI; bump it deliberately.
- **GPU backend, first slice:** `core/gpu` (chitrakar-gpu) draws solid
  fills — rectangles (rounded too), ellipses and paths, compound ones
  included — through nested group transforms, in painter's order, with
  per-layer opacity, composited premultiplied in linear light on a
  four-sample Rgba16Float target. Rectangles and ellipses take their
  coverage from their own signed distance, so an edge is as smooth as the
  pixel it lands on; a path is stencilled (a fan over its rings flips the
  stencil, so even-odd falls out of the parity and a hole is a hole
  however the ring is wound) and covered, and the multisampling softens
  it. A placed image is a textured quad whose texels are premultiplied
  into linear light before upload, so the filtering happens where the
  compositor works; shrunk — where the CPU box-filters the texels a pixel
  covers — it hands the page back rather than aliasing. A gradient fill —
  linear or radial, on any of those shapes — is a ramp baked into a row
  of 512 texels and sampled across the shape's own normalized box, so it
  follows the shape the way the CPU's does, and the layer's opacity
  scales it in the fragment. A stroke is an inner band on a rect or an
  ellipse, measured from both rims so stroking one never grows its
  bounds; on a path it is the very region the CPU tests a sample
  against, laid down as geometry — a band per segment, a disc where an
  end or a corner is round, a polygon where one is squared, bevelled or
  mitred — and unioned in the stencil, so caps, joins and a width that
  swells and tapers all fall out of the one region
  (`chitrakar_render::stroke_pieces` states that region once, so the two
  renderers cannot drift; a dashed stroke is its pieces, so the GPU
  breaks a line up the way the CPU does). Text is the whole block rasterized
  to coverage at the size it is seen at — by `chitrakar_render::
  text_raster`, which the CPU path calls too, so both read the same
  bitmap — and drawn as a quad over the block's box. It declines
  anything else — masks, effects, filters,
  adjustments, blend modes, a group that needs isolating, ink authored
  for a press (a gradient stop included), and anything wanting a texture
  bigger than the 2048 every adapter guarantees — and the caller falls
  back to the CPU. Its tests render the same page both ways and compare: mean channel
  difference under 0.004 for the analytic shapes, the gradients and the
  strokes, 0.012 for the stencilled paths and 0.0005 for text, interiors,
  holes and bare page exact, the antialiased edges tracking the CPU's. Nothing depends on it yet: the engine still
  renders on the CPU. On llvmpipe (a CPU driver, so this measures plumbing
  rather than a graphics card) a 1280×720 page costs ~22ms against the CPU
  renderer's ~8ms; CI installs mesa-vulkan-drivers so the comparison runs
  there too.
- **Next up (rough priority):**
  1. Wire the GPU backend into the engine behind a feature and let the
     viewport present from it; what is left to teach it first is masks,
     effects, adjustments and blend modes (see
     docs/spikes/gpu-rendering.md).
  2. Mobile shells: `tauri android init` / `ios init` (needs SDKs, so it
     wants a machine with Xcode/Android Studio).
  3. Depth: another review pass over the last stretch of commits (each
     pass so far has found real defects), then whatever the next user
     of the editor misses first — artboards, a brush that paints pixels.
- **Chrome:** "?" (or View › Keys and gestures) opens a sheet of every
  key and gesture, since half of what this editor can do is a gesture
  nobody would guess at. Document actions live in a File/Edit/Page/View menu bar — Edit
  carries cut/copy/paste/duplicate/delete and select-all beside undo, Page
  the page's own size, its turns and its mirrors, View
  fit/zoom/actual-size/zoom-to-selection and the guide toggles, so
  none of it depends on knowing the shortcut; the tool
  rail, layer actions and top-bar toggles are icons from `app/src/icons.tsx`
  (one stroke weight, one 24-unit grid, drawn in currentColor). Accent is
  reserved for state — active tool, open menu, live toggle, selected layer.
  Tools have single-letter shortcuts (V/M, R, E, P, B, N, S, T), suppressed while
  typing. Layer rows carry a picture of the layer (a kind glyph where
  there is nothing to picture) and a mask marker.
- **Tooling:** `tools/chitrakar-plugin/` is a Claude Code plugin bundling
  the verification gate, status, ship, the engine conventions skill, and a
  SessionStart hook (install: `/plugin marketplace add
  SandeepSubba/Chitrakar`, `/plugin install chitrakar@chitrakar`).
- **Known limits, deliberately:** the in-app clipboard carries layers
  between documents; out to other applications a selection goes as a
  picture (Edit › Copy as image puts a PNG of its box on the system
  clipboard), and images from other applications come *in*, by paste or
  by dropping a file on the canvas (a dropped .chitra opens); effects
  come in three kinds;
  and export flattens them like everything else; SVG export sets each
  line the renderer set — wrapped, aligned by text-anchor, on its real
  baseline, at the em the face is scaled to — though a synthesized
  italic lean and a loaded font are the reader's to supply; a mask is an ellipse, a rectangle, or
  another shape handed down to the layer below, moved and resized on
  canvas but not reshaped there; PDF export is live where PDF has the
  words (paths, solid fills and strokes, groups, images, opacity, blend,
  text in embedded faces, subset to the glyphs used) and the engine's
  pixels where it does not (gradients, effects, masks, varying strokes;
  an adjustment or filter flattens what is under it), and TIFF is the
  composite; a boolean
  operation flattens curves to line segments and declines outlines that
  touch or overlap exactly, rather than guessing.

---

## 1. Tech stack

| Piece | Choice | Why |
|---|---|---|
| Engine | **Rust** (`chitrakar-core` workspace) | Memory-safe, fast, compiles natively for all 6 targets *and* to WASM; one engine codebase forever. |
| App shell | **Tauri 2** | Single shell framework covering desktop *and* iOS/Android; native menus, file dialogs, small binaries. |
| UI | **TypeScript + React** (webview) | One UI codebase across all platforms; responsive layout adapts desktop ⇄ tablet ⇄ phone. |
| GPU rendering | **wgpu** (vectors via **vello**, raster ops via compute shaders) | Portable over Vulkan/Metal/DX12/GLES — and over WebGPU when the engine runs as WASM. |
| Color management | **ICC-based CMS**: `lcms2` (battle-tested) with `moxcms` (pure Rust) evaluated as the WASM-friendly alternative | Correct RGB/CMYK conversions, monitor profiles, soft proofing. |
| Codecs | `image`/`zune` crates (PNG, JPEG, TIFF), `resvg`/`usvg` (SVG import), custom exporters | Pure Rust ⇒ works on every target including WASM. |

### How the engine reaches the screen

The engine is one Rust crate compiled two ways:

- **WASM build (MVP path):** the engine runs *inside* the webview and renders through
  WebGPU (fallback: WebGL2/canvas readback). UI ⇄ engine calls are plain in-process
  bindings — no IPC serialization on the hot path. This works identically in every
  Tauri shell and keeps one render path to debug.
- **Native build (optimization path, later):** the same crate runs in the Tauri host
  process rendering with wgpu directly to a native surface composited with the webview.
  We switch per-platform only where WASM/WebGPU proves insufficient (likely candidates:
  older Android webviews, very large documents).

**Risk to validate first (Phase 0 spike):** WebGPU availability in each platform's
webview (WKWebView on iOS, Android System WebView, WebView2, WebKitGTK). The fallback
ladder (WebGL2 → software render + blit) must be proven before we commit the MVP to it.

---

## 2. Document model (the heart of the app)

```
Document
├─ metadata: color mode (RGB | CMYK), working profile, dpi, dimensions
├─ resources: embedded source images (immutable, content-addressed)
└─ root: Group
   ├─ VectorObject      — path/shape parameters, fills, strokes (all editable)
   ├─ RasterObject      — reference to immutable source pixels + its own
   │                       non-destructive edit stack (crop, transform, adjustments)
   ├─ AdjustmentLayer   — curves, levels, HSL, exposure… applies to everything below
   ├─ FilterEffect      — gaussian blur, sharpen… attached to an object or a group
   ├─ Group             — nesting, blend mode, opacity, clipping
   └─ Mask              — raster or vector mask attachable to any node
```

Key rules:

- **Source pixels are immutable.** A RasterObject points at a resource; edits are
  parameter stacks evaluated at render time.
- **Rendering is a pull-based graph evaluation** with per-node caching: a node re-renders
  only when its parameters or inputs change. Caches are tiled (e.g. 256×256 tiles) so
  editing one region doesn't invalidate the whole canvas.
- **Edits are commands.** Every mutation goes through a command object → free undo/redo,
  and later a path to collaborative editing (commands are serializable).
- **Working pixel format:** 32-bit float, premultiplied, linear light, in the document's
  working space. Blending happens in linear; display transform is the last step.

### File format: `.chitra`

A ZIP container (same family as `.ora`/`.sketch`):

```
document.chitra
├─ manifest.json     — versioned schema: full node tree + parameters
├─ resources/        — original embedded images, untouched bytes
├─ profiles/         — embedded ICC profiles
└─ thumbnails/       — preview renders
```

Human-diffable manifest, originals preserved byte-for-byte, forward-compatible via
schema version + "unknown node" passthrough (unknown future node types survive
open→save round trips).

---

## 3. Color pipeline (RGB + CMYK)

No GPU API understands CMYK — so the CMS is part of the engine, not the platform.

```
import:  decode → assign/honor embedded ICC profile → convert to working space
edit:    all compositing in linear float, working space
display: working space → monitor profile (or soft-proof: working → CMYK press
         profile → monitor, with gamut warning overlay)
export:  working space → target profile (sRGB PNG/JPEG, CMYK TIFF/PDF), profile embedded
```

- **RGB documents:** working space = linear form of the chosen profile (sRGB default;
  Display P3 / Adobe RGB selectable).
- **CMYK documents:** native CMYK values are preserved on objects where they were
  authored (a "C:100 M:0 Y:0 K:0" fill stays those numbers); compositing happens in a
  linear RGB proxy space with the document's press profile (e.g. FOGRA39, GRACoL)
  driving display and export. This is the Affinity/Photoshop-style compromise that keeps
  editing fast *and* output correct.
- Soft proofing and per-document rendering intent (perceptual/relative colorimetric)
  are first-class UI, not buried settings.

---

## 4. Repository layout

```
chitrakar/
├─ core/                  # Rust workspace
│  ├─ doc/                # document model, commands, undo, .chitra I/O
│  ├─ render/             # the CPU renderer: the correctness reference
│  ├─ gpu/                # the wgpu backend, validated against render/
│  ├─ color/              # CMS wrapper, profiles, pixel formats
│  ├─ codecs/             # import/export (PNG, JPEG, TIFF, SVG, PDF)
│  └─ engine/             # public API: the one crate the shells embed
│                         #   (cdylib for native, wasm-bindgen for WASM)
├─ app/                   # TypeScript UI (React) — tools, panels, canvas host
├─ shells/tauri/          # Tauri 2 config for desktop + iOS + Android
└─ docs/                  # this plan, ADRs, format spec
```

---

## 5. Roadmap

### Phase 0 — Foundations & risk spikes (small)
- ✅ Scaffold Rust workspace, Tauri 2 app, React UI, CI (fmt/clippy/test + desktop builds).
- ✅ Desktop packaging: app icons generated for every platform, bundling enabled
  (deb/rpm/AppImage, dmg, msi/nsis), a Linux .deb built and inspected locally,
  and a release workflow producing installers for Windows, macOS (Intel +
  Apple Silicon), and Linux on version tags or manual dispatch.
- **Spike 1:** WASM engine + WebGPU triangle→texture inside Tauri webview on desktop,
  iOS Simulator, Android emulator. Decide the fallback ladder with data.
  - ✅ *First half proven:* engine compiles to WASM (wasm-bindgen), runs in-browser,
    renders to canvas via `putImageData`; full editor loop (draw/undo/hide/save)
    verified headless in Chromium. WebGPU-in-webview per platform still open.
  - ✅ *Native wgpu proven headless* (docs/spikes/gpu-rendering.md): wgpu 23 on
    llvmpipe software Vulkan renders pixel-correct at ~3ms per 1280×720
    draw+readback — the GPU backend is developable and CI-testable against
    the CPU reference renderer.
- **Spike 2:** ✅ lcms2 vs moxcms — **moxcms chosen** (compiles to wasm where
  lcms2's C core cannot, ~4.4× faster, matches lcms2 within 1/255 on RGB and
  CMYK press-profile transforms). Full numbers: docs/spikes/color-management.md.

### Phase 1 — Core editor (vector + raster objects)
- ✅ Document model, command/undo system, `.chitra` save/load (manifest-only container;
  embedded resources arrive with raster support).
- Cached incremental rendering ✅: the engine keeps a composite cache, computes
  dirty regions from node bounds per command, and re-renders/re-encodes only
  those pixels (adjustment layers dirty everything below, by design). Per-node
  tile caches refine this later. Canvas pan/zoom ✅ (wheel zoom toward cursor,
  space/middle-drag pan, fit-to-window).
- Live gestures ✅: preview/commit/cancel in the engine — drags update the
  document each pointer move, history records one undo step per gesture,
  Escape cancels. Transforms support scale (shear/rotation with the GPU path).
- Vector: rect/ellipse ✅ drawn interactively; polygon paths ✅ — even-odd
  fill, centered stroke for open polylines (line art), hit testing and
  stroke-aware bounds — drawn with the pen tool ✅ (click anchors, click the
  first anchor to close as a filled shape, Enter finishes an open stroked
  path, Escape abandons, dashed live preview). Bezier segments and anchor
  editing pending; gradient fills pending.
- Raster: place PNG/JPEG as RasterObject ✅ (content-addressed resource pool,
  pixels embedded as PNGs in .chitra, undoable placement, move + hit test);
  scale/rotate pending.
- Layer panel: hide ✅, select ✅, delete ✅, reorder ✅ (MoveNode command:
  reorder + reparent with subtree-cycle protection), opacity slider ✅,
  blend-mode picker ✅; grouping UI pending.
- Selection ✅ (hit test + move tool with live preview); corner resize
  handles ✅ (anchored scaling); rotation handles pending.

### Phase 2 — Non-destructive power
- Adjustment layers: brightness/contrast ✅, exposure ✅, hue/saturation ✅
  (feColorMatrix-style hue rotation + luminance-relative saturation) — all
  re-editable via the properties panel with live slider preview, one undo
  step per gesture. Levels ✅ (input range, gamma, output range, in linear
  light) and Curves ✅ (monotone cubic through points in the display
  encoding, tabulated once per pass; press-to-add-and-drag graph editor;
  a master curve plus one per channel run after it — colour grading —
  with a channel picker and the untouched channels drawn behind).
- Vector styling ✅ first pass: fill and inner stroke (color + width),
  editable on existing objects; stroke-only shapes hit-test on the band.
  Gradients pending. Layer rename ✅ (SetName command, inline edit).
- Filter effects: gaussian blur ✅ and sharpen (unsharp mask) ✅ as
  non-destructive layers — CPU path uses three iterated box blurs
  (O(pixels) per pass, W3C feGaussianBlur approximation) in premultiplied
  linear; parameters live-edit via the panel. While any filter layer exists,
  incremental invalidation falls back to whole-canvas (neighborhood reads at
  region edges); padded region rendering and the GPU compute path refine
  this later.
- Masks ✅ first pass: a mask attaches to any node — vector masks (hard shape
  coverage) and raster masks (luminance × alpha, transform-sampled) modulate
  a shape's/image's paint, a group's composite, and an adjustment's or
  filter's strength; invert supported; UI adds an inscribed ellipse mask
  with invert/remove. Clipping ✅: a layer confined to the one below it
  shows only where that one does and is hidden with it, a run of them
  stacking against the same layer; an adjustment, a filter or a clone —
  which are changes to what is under them rather than pictures of their
  own — is applied where it stands and mixed back by the confinement
  instead. SVG carries it as a mask of the layer below, PDF as pixels.
- Full undo/redo history panel ✅: every edit records a human-readable label
  (from the forward command and the touched layer's name); the panel lists
  past and undone-future edits and clicking jumps the document to that point.
- Grouping ✅: Batch command (atomic multi-command with rollback, one undo
  step); group ctrl-click-selected same-parent layers into a new group,
  ungroup dissolves in place — both single history entries.

### Phase 3 — Color management & export
- ICC import honoring embedded profiles ✅ (PNG/JPEG pixels tagged with an
  RGB profile normalize to sRGB at the decode edge via moxcms).
- CMYK document mode ✅ with press profiles ✅: documents carry an ICC press
  profile (persisted in .chitra as profiles/cmyk.icc, loadable in the UI);
  authored CMYK ink renders through it, naive formula as fallback; shapes
  drawn in CMYK documents author real ink values with C/M/Y/K ink sliders ✅.
- Soft proofing ✅ + gamut warning ✅: display-only round trip through the
  press profile at the presentation-encode step (exports stay unproofed);
  out-of-gamut pixels mark neutral grey. Monitor profiles and rendering-
  intent selection pending.
- Export: PNG ✅ (sRGB composite), SVG ✅ (live vector markup — shapes,
  paths, groups with opacity/blend, embedded rasters, text; a mask
  travels as a picture of what it lets through — white with the coverage
  in its alpha, which reads the same whether a consumer takes a mask by
  luminance or by alpha — on a wrapper carrying no transform, since SVG
  reads a userSpaceOnUse mask in the space in force where it is
  referenced; CMYK colors
  resolve through the press profile; adjustments and filters noted as
  omitted), CMYK TIFF ✅ (composite separated into ink through the press
  profile, composited over paper white, 4-channel TIFF with that profile
  embedded; refuses rather than guessing when no profile is loaded).
  and PDF ✅ (one page sized from the document dpi; live paths, solid
  fills and inner/centred strokes, nested groups, image XObjects with
  soft masks, opacity and blend as graphics states, text as text (each
  face embedded once as a CID font addressed by glyph id, glyphs where
  the shaper put them so kerning and ligatures survive, a synthesized
  italic as a text-matrix skew, a synthesized bold as a stroke around the
  filled glyphs, and a ToUnicode map so the words can be
  found and copied — checked by reading them back through Ghostscript;
  the face travels as a TrueType subset of the glyphs used, ids kept),
  and the engine's pixels — oversampled towards 300 dpi and trimmed to
  their ink — for what PDF cannot say; with a press
  profile authored ink is written as ink, sRGB is separated through the
  profile, and the profile is both the ICCBased colour space and the
  page's output intent; a Ghostscript test checks the page against the
  CPU renderer). JPEG ✅.

### Phase 4 — Mobile shells
- Tauri iOS/Android builds; responsive UI ✅ first pass: below 900px the
  layer panel comes over the canvas rather than beside it, asked for from
  the bar, and the bar sheds what is only informational so the window
  holds all of it. Bottom toolbars still to come.
- Touch + Apple Pencil/stylus input (pressure into the input pipeline early, ahead of
  brush tools). Pressure ✅ (a pen's own reading drives the brush's width,
  with a mouse's speed standing in for it); the view's own gestures ✅
  (two fingers pinch to zoom and carry the page, and take the gesture
  over from the one that began it).
- Platform file integration (Files app, Android SAF, share sheets).

### Phase 5 — Depth (ongoing)
- Pen tool + full path editing; boolean operations on shapes.
- Text objects ✅ first pass: live TextSpec nodes (string, size, color as
  document state; glyphs rasterize at render time via ab_glyph + bundled
  DejaVu Sans, kerned per-glyph layout with newline support), blitted through
  the node transform with mask/opacity/blend support; Text tool click-places,
  panel edits content/size/color with gesture preview; resize handles work.
  Proper shaping (`rustybuzz`/`parley`), font choice, and weights pending.
- Brush engine for raster painting ✅ first pass: a paint layer holding
  live strokes (line, per-point radius, colour, soft edge, erase), each
  one removable. Clone and heal ✅ as non-destructive ops: a clone layer
  holds strokes that read the page at their own offset, so what they lay
  down follows the source when it changes.
- Artboards ✅ first pass: `NodeKind::Artboard` is a group with a size of
  its own that grounds and cuts its contents, takes what is drawn inside
  it, and exports one PNG per frame at the frame's own size; live in SVG
  (a clipPath) and PDF (a rectangle clip). Resized by its handles or its
  W/H fields rather than scaled, with a ground that can be any colour or
  none, and what is inside pinned to its edges, its middle or both.
  Frames as export presets still to come.
- Symbols/components ✅ first pass: `NodeKind::Instance` draws another
  layer's content in its own place, with a cycle guard in the document
  (any structural command that would let a copy reach itself is refused
  and rolled back), and a copy can stand in for the original's direct
  children with layers of its own. Live in SVG and PDF. Standing in for
  a layer deeper than the original's own children is still to come.
- Live effects (drop shadow, outline), styles.
- Later bets enabled by the architecture: collaboration (serializable commands),
  plugin API (WASM sandboxed), web build (engine already compiles to WASM).

---

## 6. Guiding decisions (mini-ADRs)

1. **One engine, two compilations (WASM + native)** — never fork the engine per platform.
2. **Linear float compositing** — correctness first; 8-bit preview paths only as a
   measured optimization.
3. **Tiled, cached, pull-based rendering** — the non-negotiable for non-destructive
   editing at interactive speed.
4. **Immutable sources + parameter stacks + commands** — undo, history, and future
   collaboration all fall out of this one choice.
5. **ZIP+JSON container format** — inspectable, versionable, resilient; binary-only
   formats are a trap at this stage.
6. **UI in the webview, pixels in the engine** — the UI never touches pixel buffers;
   it sends commands and displays engine-rendered textures.
