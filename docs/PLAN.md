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
  is; a curves layer takes each channel to its own ends on one press,
  which is what pulls a colour cast out of a picture without being told
  what in it is meant to be grey — a photograph under a blue light has a
  blue channel reaching further than its red, and stretching each to
  where its own tones stop is the same picture without the light (two
  points a channel, the plainest curve that says it, and a channel with
  nothing to stretch written as nothing at all);
  white balance can be taken from the picture rather than typed —
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
  hue, saturation and lightness asked of one band of colour at a time —
  the reds, the yellows, the greens, the cyans, the blues, the magentas —
  which is how a sky is deepened without touching the grass, or a face
  warmed without warming the wall behind it: a pixel belongs to the bands
  its own hue falls between, by how near it is to each, and the weights
  are a triangle a band wide so they always add to one and no colour sits
  in a seam; how much of the change it takes is how much colour it has,
  fading out towards grey, though a third of full saturation already
  takes all of it, since a pale sky is still a sky;
  shadows and highlights, which moves the two ends of the tone range and
  leaves the middle where it is — the first thing asked of a photograph
  after exposure, since a face against a window is dark because the
  window is bright and no single exposure fixes both; each end's pull
  falls off as the cube of the distance from it, what moves is the
  pixel's brightness and its colour comes along, and both run the other
  way for deepening a shadow rather than lifting one (a function of the
  pixel alone: Photoshop's reads the neighbourhood too, which is where
  its local contrast and its halos come from);
  colour balance, the three ranges of tone pushed along the three
  opponent pairs — cyan/red, magenta/green, yellow/blue — which is how a
  print is corrected and how a grade is given its colour: cool shadows
  against warm highlights, said in the terms the correction is thought
  in rather than as three channel numbers. Which range a pixel belongs
  to is read from its lightness rather than per channel, so a shift
  moves a colour instead of pulling it apart, and the three masks are
  ramps that add to one, so no tone sits in a seam between them; holding
  the brightness (on by default) puts the pixel's own lightness back
  after the colour has moved, so correcting a cast does not also lift
  the picture;
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
  a number is taken out of a picture with — a motion blur, which is a
  smear along one direction rather than a spread in every one, and so is
  what a camera does to a moving thing and what a still picture is given
  to say the same; noise, which is grain, and a
  vignette, which takes the corners of the page down to hold the eye in
  the middle of a picture, or lifts them to take one a lens put there
  back out; it is measured from the middle of the page in document
  units, so panning slides the picture under it rather than carrying it
  along, and it scales the three channels and leaves alpha alone, so it
  darkens the colour without touching what is covered)
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
  documents) — every one of those, and the four ways of reordering,
  reaches everything picked rather than
  the layer the panel happens to be showing, in one history entry named
  after what it did and how many it touched ("Delete 3 layers", not
  "Multiple edits"), and
  what lands is what is picked afterwards, since a paste or a duplicate
  is usually about to be moved somewhere; the clipboard carries the
  layers in the order the document held them, leaving out any that
  travel inside another of them, and a copy here takes the system
  clipboard with it (the layers' names go on it as text), because a
  picture copied in another application sits there until that
  application lets go of it and a paste prefers a picture — so one old
  picture used to win every paste for the rest of the session, however
  many layers had been copied since — copy and paste a layer's look on its own — what it is
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
  is laid level against the edge of the page by eye — or drawn along:
  the panel sits over the canvas rather than in front of it, and a line
  dragged along anything that ought to be level or upright (a horizon, a
  doorframe, the edge of a table) sets the angle from however far off it
  is, taking it for whichever of the two it is nearer; the whole gesture
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
  after it, with nothing of the page around it — or at twice or three
  times that, since a frame carries the multiple it wants exporting at
  the way it carries its size: which screen a frame is for is a property
  of the frame rather than a thing to remember at the moment of
  exporting, and the file is named `@2x` the way every export of a screen
  has been named since screens had two of them. A file written before
  frames could say reads as one to one; File › Export PDF of the
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
  multi-selection — dragged out as a band over empty canvas, built
  shift-click by shift-click on the canvas, or ctrl-click by ctrl-click
  down the panel (shift is what adds on the canvas because ctrl is
  spoken for there: it drags free of the snapping, and a modifier cannot
  mean two things about one gesture; a shift-click takes a layer out
  again as readily as it puts one in, the primary included) — moves as one (alt-dragging takes a copy and
  leaves the original), by drag or by
  arrow key (shift for a coarse step) — in a single history entry. Exact
  placement is typed: the panel carries X/Y/W/H in document pixels, and
  the angle beside them — the knob turns a layer by eye, and some things
  have to be at forty-five degrees exactly — with the two sides tied
  together by default, so typing a width takes the height with it the way
  a dragged corner does, and a chain between them to untie them. Typing one turns the layer
  about the middle of its own box, which is where the knob turns it, so
  the number says where it stands rather than moving it.
  The document keeps a palette of its own: colours added from whatever
  is being drawn with, saved with the file, clicked to draw with and to
  give to the picked shape or block of text, alt-clicked to take out
  again. In a CMYK document they are ink, and resolve through the press
  profile exactly as a fill does. A colour reached for in the palette
  *stands for* the entry rather than being a copy of it — the palette
  marks the entry the picked layer reaches for, and the colour stays in
  hand, so the next shape drawn reaches for it too — and shift-clicking
  an entry to say what it now means recolours everything that reached
  for it. Which is what makes a palette a set of decisions rather than a
  set of colours kept handy: one place to change a brand blue, not a
  dozen layers to find. A named colour carries what the name means as
  well as the name, so it is still a colour away from the document it
  was authored in — exported, or in a palette the name has since been
  taken out of, where the layer keeps the colour it was drawn in.
  A right-click on the canvas offers what can be done with what is under
  the pointer, where the pointer is: cut, copy, duplicate, the front and
  the back, lock and hide, group and ungroup, delete, and the one thing
  the canvas could not do at all — pick the group a layer is in, since
  hit testing only ever reports the leaves and a group was reachable
  only from the panel — and on bare canvas the things
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
  back throws it away. A grid of eight, sixteen or thirty-two pixels can
  be laid under the artwork, and its lines catch a drag exactly as the
  page's edges and the other layers' do — they are listed as lines like
  any other rather than solved for, so nothing about the snapping has to
  know a grid from a guide. It is a view setting: it says how you are
  working rather than what the document is, so it is remembered between
  visits and saved with nothing. A grid finer than the screen can draw is
  not drawn at all, since a grey wash is not a grid.
  The view has the keys every editor has: Ctrl+= and Ctrl+- zoom about
  the middle of the window, Ctrl+0 fits the page to it and Ctrl+1 shows
  the page's own pixels one for one, and the zoom is read and set in the
  same place — "show me this at four hundred percent" is a thing people
  say and a wheel cannot answer it, so the figure in the bar is a field.
  It sits outside the chip beside it, since that chip goes when the
  window is narrow and this is a control rather than a caption.
  A window too narrow to hold a column of layers beside the canvas — a
  phone, a tablet held upright, a window dragged small — lays the panel
  over the canvas instead, out of the way until the bar's own button asks
  for it, and the bar drops what only tells you something (the name over
  the door, the size of the page) so that nothing hangs off the side and
  slides the whole page about.
  On a device that says its pointer is coarse, every grab on the canvas
  is finger-sized — corners, the knob a layer turns by, a path's anchors
  and their handles, a mask's and a gradient's, and the width of a guide
  worth catching. Each is placed by its own middle rather than by its
  corner, so how big it is belongs to the stylesheet alone and it sits
  on the thing it moves at any size.
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
  so a closed tab or a crash loses nothing. Work that has not been saved
  is said so — a dot beside the name — and is not thrown away without a
  question: starting another document or opening one asks first, and the
  browser asks its own question on the way out of the tab. The draft is a
  net for a crash and not for this, since starting another document
  overwrites it a breath later; undoing back to where the document was
  last saved counts as saved again, because that is what a person means
  by it. The document has a name —
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
  soft proofing + gamut warning, and the screen's own profile — everything
  shown is taken from sRGB to that display's numbers, so a wide-gamut
  monitor draws the picture as it is rather than as far out as its own
  red will go. It is the last thing applied to what is shown and applied
  to nothing else: not the document, not an export, not a colour picked
  off the page, and it is not saved with the file, because it belongs to
  the machine rather than the picture. A profile is loaded from a file,
  or Display P3 is offered outright, since most of what Apple ships is
  P3 and few people have the .icc to hand. Files: `.chitra` save/open; export PNG (at 1x, 2x or 3x, or of just
  the selection — rendered at that size, not upsampled), JPEG, SVG,
  CMYK TIFF, PDF. Every one of those goes through one helper, so a save
  or an export that cannot be made says which one and why rather than
  quietly producing no file — a failure that says nothing looks exactly
  like a browser that refused the download. The New-document dialog is
  honest about its ceiling the same way: the fields keep the number that
  was typed, and if a side is over 8192 the dialog says what it will
  make instead (it used to clamp inside the field on every keystroke,
  so asking for 30000 got 8192 with nothing said, and a big number could
  not be typed at all). Desktop app packages (deb verified locally; CI builds
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
  million times, which took a full-page map from 1.48 s to 0.95 s, and
  then read off into a table of its own (`RampLut`, a thousand entries),
  which took it from 835 ms to 389 ms. Walking the stops per pixel meant
  finding the pair a tone falls between and mixing them in the display
  encoding — six crossings of the transfer curve and three back, every
  pixel — where a ramp is a curve of one variable and can be read off
  once; the GPU bakes its own row of texels for the same reason, and a
  test holds the table to the ramp everywhere rather than only at the
  entries it was built from. The transfer curve is tabulated for the
  places that need to know where a tone sits and not for the ones that
  take a value over and bring it back: a round trip through a table is a
  hair short of exact, and at eight bits that showed up as a level on a
  channel a curve had not been drawn for.
  Resolving it there is also where a CMYK document's stops meet its
  press profile, which is the last place the document is to hand.
  A blend is the dearest thing left on the page: it crosses that curve
  nine times a pixel — six going in and three back — and a probe beside
  the page one says what a single pixel of it costs, away from the
  rasterizing around it. Two changes took a full-page multiply at A4
  300dpi from 1.24 s to 0.97 s (86 ns a pixel to 63): the three channels
  on each side are divided by the same alpha, so that is two reciprocals
  and six multiplies rather than six divisions; and the transfer table's
  length is part of its type, so the index — held to the last entry
  before either end of the span is read — is provably inside it and the
  nine reads cost no bounds check.
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
- **The sheet of keys is a promise:** `?` opens a list of what the app
  says it does with the keyboard and the pointer, and a line of it that
  the app does not honour is a bug of the worst kind — the
  multi-selection it claimed on the canvas was missing for months
  because the panel's own version of it was well tested and nobody asked
  the canvas. The suite now presses every letter the sheet names and
  asks the rail what it is holding, and drives the gestures it promises
  that no other block covers: both ways of carrying the view, letting go
  of a selection and picking all of it, and adding to one with a band.
  Add the test with the line when the sheet grows.
- **Verify before committing:** `cargo test --workspace` (~401),
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all`,
  and in `app/`: `npm run build && npm run test:e2e` (~1004 browser
  assertions; while writing one, `node e2e/one.mjs <block>` runs a single
  block against the harness alone, in seconds rather than the quarter of
  an hour the whole suite takes — the suite is still the gate). Both
  suites self-skip CMYK-profile steps unless
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
  bitmap — and drawn as a quad over the block's box. A masked layer is
  held to its mask: the coverage comes from
  `chitrakar_render::mask_plane_over` — the same reading the CPU
  compositor does at every pixel of a masked layer, so a mask cannot come
  to mean two things depending on which renderer drew it — rasterized
  over the layer's own box, uploaded as a one-channel texture, and
  multiplied into every fragment, whichever kind of draw it belongs to
  (shape, stencilled path, stroke, text, image, gradient). Every mask
  works, cut from a shape or brushed on by hand, inverted or not, since
  all of them are that one reading. A mask on a *group* is a different
  thing: it holds what the group composites to, and holding each child
  to it instead would take the coverage twice where two of them overlap.
  So a masked group is drawn on a surface of its own, and the mask holds
  the one quad that lays that surface down. So is a group at less than
  full opacity, for the same reason — the layers inside it meet each
  other at full strength and the result comes down together, which is
  not what taking each of their opacities down would give. A pass has
  one set of attachments, so this means cutting the work into passes at
  every group that composites as a unit: one surface per depth of
  nesting, reused by every group at that depth, since a group's surface
  is laid down the moment its contents are finished and nothing reads it
  after. The multisampled attachment is kept only when its surface is
  drawn on again, so a page without such a group costs exactly what it
  did. A layer with a blend mode is drawn on one of those surfaces for
  the same reason and brought down by a fragment that works out the
  whole answer — all sixteen modes the W3C spec names, on the values a
  device shows, exactly as the CPU compositor reads them — and writes it
  over what was there rather than blending into it. What it is coming
  down onto is read from a copy taken just before the pass, since a pass
  cannot sample what it is drawing into; one copy serves the page, since
  the passes run in order and it is spent before the next begins. Every
  mode is held against the CPU's own answer, and where there is no layer
  the answer is what was already there, so the rest of the page comes
  through untouched. An adjustment layer works the same way and reads the
  same copy: it rewrites everything composited below it, weighted by its
  own opacity and its mask, with the arithmetic stated arm for arm as
  the CPU states it — some of it in linear light, some on the values a
  device shows, which is a decision that belongs to the adjustment
  rather than to the renderer drawing it. All thirteen of them: the nine
  stated by a handful of numbers ride on the quad, and the three read off
  a table — the curves, the gradient map's ramp, the six bands of a
  selective adjustment — read the table the CPU renderer itself builds,
  uploaded beside the quad, so neither renderer can read a table the
  other did not write (which is also how a CMYK document's press profile
  reaches a gradient map, since only the prepared ramp knows it). A group
  holding something that
  reads what is under it is isolated too, because that is what decides
  what "under it" means, and the CPU renderer asks the same question
  (`chitrakar_render::reads_backdrop`), so both give the adjustment the
  same page to work on. A filter layer that is a function of one pixel and of where
  that pixel sits on the page — a vignette, a field of grain — is an
  adjustment in every way this backend cares about, and rides the same
  machinery; the grain is the CPU's own hash, arithmetic for arithmetic,
  so both renderers speck a page the same way. A blur is six box passes on a pair of scratch
  textures — three along each axis, which is the CPU renderer's
  Gaussian written out as render passes — and a sharpen is the same
  blur read as what the picture has too little of. A layer held to the one under it shows only
  where that layer's own alpha does — which is that layer drawn aside,
  so it arrives the way a mask does, as a coverage in a texture, and the
  two renderers cannot come to disagree about clipping either; a layer
  that is both held and masked is held back by one coverage, since two
  of them multiplied are one. Only where "its alpha" is a plain question
  though — a base that draws a picture of its own, at full strength,
  unblended and unmasked; an adjustment has no picture and a faded or
  blended base has an alpha that depends on how it was composited. A frame is a group with a size of its own: its ground is
  the rectangle it is, filled, and everything under it is held to that
  rectangle — rounded to whole pixels there as here, because a frame's
  edge is a page edge and a page edge is crisp, which also means the
  holding takes all of a pixel or none and can ride the same coverage a
  mask does rather than needing a surface of its own. Upright and
  composited like its contents, a frame is nothing but a narrower
  region to paint in, which is how the CPU renderer reads it too — so
  an adjustment inside a frame sees the page below the frame on both.
  A copy of another layer draws what that layer draws, where the copy
  is: the original's placement is undone and the copy's applied, so
  moving the original moves only the original, and where the copy
  stands in for the original's own children with layers of its own,
  those are what it draws. Drawing one layer where its parent puts it
  is its own function here now (`one`), which is what let a copy ask
  for the thing it is a copy of without a parent to have walked down
  from.
  It declines
  anything else — effects; a layer held to a base like that; a frame
  that is turned, composited as a whole, or whose box does not land on
  whole pixels, each of which the CPU draws another way; a copy of
  another layer that is faded, blended or masked, for the same reason;
  a paint layer; pixelate (whose neighbourhood is a block
  rather than an axis); ink authored
  for a press (a gradient stop included); and anything wanting a texture
  bigger than the 2048 every adapter guarantees — and the caller falls
  back to the CPU. What is declined is checked as carefully as what is
  drawn, because declining always gives the right page slowly and
  drawing the wrong thing never does: `whatever_the_gpu_agrees_to_draw_
  it_draws_the_way_the_cpu_does` walks the shared fixture's every
  `Command`, one at a time, and holds the backend to the CPU's answer
  wherever it agreed to draw at all. It found clipping on the way in —
  a layer held to the one below it was being drawn whole, which is not a
  slower picture but a wrong one — and it is what a new `Command` or a
  new node kind runs into. The node kinds this backend cannot draw come
  out of that fixture first, since one of them in the document declines
  the whole page and an audit declined every time measures nothing; as a
  kind is learned, its line goes and the commands that speak to it come
  into scope by themselves. Its other tests render the same page both
  ways and compare: mean channel
  difference under 0.004 for the analytic shapes, the gradients and the
  strokes, 0.012 for the stencilled paths and 0.0005 for text, interiors,
  holes and bare page exact, the antialiased edges tracking the CPU's. Nothing depends on it yet: the engine still
  renders on the CPU. On llvmpipe (a CPU driver, so this measures plumbing
  rather than a graphics card) a 1280×720 page costs ~22ms against the CPU
  renderer's ~8ms; CI installs mesa-vulkan-drivers so the comparison runs
  there too.
- **The fixture holds one of every node kind:** it had the six that
  cover — shapes, a group, a paint layer, a picture, a block of text, a
  frame — and none of the four that draw by *reading*: an adjustment
  and a filter rewrite what is composited below them, a clone lays down
  what the page already holds somewhere else, and a copy draws another
  layer's content in its own place. Adding them found three defects in
  one run, all of the same shape and none reachable without a copy in
  the document. Changing a layer dirtied every copy *of that layer* but
  no copy of the group it sits in — which is how symbols are actually
  made, so every edit inside one left stale paint at each copy. A
  session opened from a file believed there were no copies in it until
  something structural happened, so a plain drag left them behind. And
  on the GPU path a coverage — a mask, or the alpha a layer is held to
  — was rasterized over the box the *document* places the layer at, so
  a layer inside a copied group was held back by what was happening at
  the other end of the page.
- **Every number that crosses the boundary is given one nobody could
  mean:** `a_number_nobody_could_mean_is_answered_rather_than_fallen_
  over` calls every `Session` method that takes a number with the
  largest float there is, a billion, minus a billion, NaN, both
  infinities and both zeroes — and every index-shaped one with
  `usize::MAX`. The app sends sane values; the boundary is public and
  has only the caller's word for them, and each of these numbers
  becomes an allocation or the length of a loop. The two ways that goes
  wrong are the two ways an editor disappears rather than complains: an
  allocation that fails aborts the process, and a loop primed with a
  few million is a wait nobody comes back from. So the audit is simply
  that it comes back — a failure is the test binary dying or never
  finishing, and both say what they mean. Written out, it found four at
  once: a surface whose two sides multiplied as `u32` (a panic in a
  debug build and, worse, a small buffer carrying big dimensions in a
  release one), a thumbnail size that was a surface nobody bounded, a
  padding added to a clip's edge past what a `u32` holds, and five
  places where a reach saturating into a `u32` then had one added to
  it. It asks the same of a *command*, since one carries numbers too
  and arrives as JSON from the app or out of a file — a transform with
  no thickness, a stroke wider than the world, an effect that reaches
  past it — and then renders whatever that left, because a document the
  renderer has no answer for is the same failure one step later. Those
  it found nothing wrong with, which is worth knowing.
- **And every stretch of bytes:** the other half of what arrives from
  outside is files — a picture, a drawing, a face, a colour profile, a
  document — and every one of them can be nothing, noise, or the first
  half of something real, which is what a download that stopped looks
  like. `bytes_that_are_not_what_they_claim_are_refused` hands each of
  those to each entry point and then draws whatever it left. A
  `.chitra` gets its own, since it is the one that can *lie*: the
  page's size and every resource's come out of a manifest while the
  pixels come out of entries beside it, so those sizes are numbers
  arriving from outside exactly as the app's are. The page had been
  held to what can be drawn since early on; a resource had not, and a
  file naming one of 65536 by 65536 overflowed the very check meant to
  hold its bytes against it — a panic in a debug build, and in a
  release one a wrap that lets a handful of bytes stand for an enormous
  picture.
- **Everything the page carries is carried the same way:**
  `map_page` is the one place a page transform is stated, and every
  page-space thing has to be named there or it is quietly left behind —
  the regions kept by name were, a chunk after they were added, and
  nothing complained. `everything_the_page_carries_is_carried_the_same_
  way` turns, mirrors, crops and straightens a page and checks that the
  things carried still agree with each other. It names no transform of
  its own: writing the arithmetic out a second time would only be a
  second chance to write it wrong. Instead it puts the same region in
  two of the places the page carries — as the selection and as a kept
  region — and asks whether they still cover the same points
  afterwards. Anything `map_page` forgets stops agreeing with what it
  remembers, whatever the transform was.
- **Every kind of layer, through the clipboard:** the clipboard is the
  one place a layer leaves the document it was made in, and what has to
  travel differs by kind — a picture's pixels, a mask's, a paint
  layer's strokes, an adjustment's numbers. A kind added later reaches
  that code without anybody thinking about it, and the failure is
  quiet: a layer that arrives looking right and drawing nothing.
  `every_kind_of_layer_survives_the_clipboard` sends each of the
  fixture's ten into a fresh document and holds the arrival against
  what was sent. It found a copy of another layer arriving broken: the
  copy kept the id its original had in the document it came from, which
  in a new document is somebody else's layer or nobody's. Now a copy
  whose original travelled with it points at the one that *arrived* —
  and a copy whose original stayed behind still points at it, since
  that is what duplicating a copy on its own means. One that points at
  neither is refused by name rather than pasted to draw nothing.
  Duplicating goes by the same rule and needed the same fix: it is a
  second, near-identical path (`emit_copy` beside `emit_clip`), which
  is exactly why it was missed, and there duplicating a group holding
  an original and a copy of it gave a group whose copy went on watching
  the *old* original — two things linked in a way nobody asked for.
- **Every kind of layer, into a group and back out:** grouping is the
  one edit that changes a layer's parent without changing the layer,
  and the way it goes wrong is quiet — the page looks right while the
  layers are wrapped and something is different once they are loose.
  `every_kind_of_layer_goes_into_a_group_and_comes_back` puts each of
  the fixture's ten in alone, checks the page and the node, dissolves
  the group and checks both again; the three that draw by reading what
  is under them (an adjustment, a filter, a clone layer) are asserted
  to look *different* while wrapped, because that confinement is the
  whole point of putting an adjustment in a group.
  `dissolving_a_group_answers_for_what_the_group_carried` asks the
  other half of the question, and that is where the defect was. A group
  is somewhere to put layers and also a layer in its own right: it can
  be hidden, locked, made half-transparent, given a blend mode, a mask,
  effects, a clip to the layer below. Its transform was already handed
  to its children on the way out; everything else was dropped on the
  floor. Some of it can be carried and some cannot, and the difference
  is not a matter of taste — a group is drawn by compositing its
  children onto a surface of its own and then treating that surface as
  one layer, so hidden and locked mean exactly the same thing said of
  each child, while half-transparent does not (two children overlapping
  inside a 50% group show one edge; at 50% each they show two). Hidden
  and locked are now carried — a hidden group whose layers reappear on
  being dissolved is the picture changing behind the user's back —
  and opacity, blend, mask, effects and a clip that actually bites are
  refused by name, with the page left untouched, because a page that
  quietly changes is worse than an edit that declines. Measured rather
  than reasoned: each of the seven was tried both ways over the
  fixture and the differing-pixel count read off, which is how the
  ones that look carryable but are not (a clip, an opacity multiplied
  into a child that has a drop shadow) were told apart from the two
  that are. A clip on the bottom-most layer of a parent has nothing
  under it to be confined to and is already ignored by the renderer,
  so refusing over it would be refusing over nothing: that case
  dissolves. What grouping does to a clipped layer is left as it is —
  it loses the clip, which is what every other editor does too, and
  moving the flag onto the wrapper would make the group undissolvable
  by the rule just stated.
- **A mask does not travel with the layer's own transform:** it is
  written in the space its owner is *placed* in — the parent's — which
  is what lets a layer be moved behind its mask, and is why `map_page`
  carries a layer's mask by hand along with the page. Dissolving a
  group hands the group's transform to each child and is exactly that
  kind of move, and it left every child's mask behind in the space the
  group used to occupy: a moved group, dissolved, slid its layers out
  from under their own masks. The same for a drop shadow's offset,
  which is a vector in that space. `carry_mask` was a private helper
  inside `map_page` with three callers; it is now
  `Mask::carried_through` and `Effect::carried_through` beside it, so
  the rule is stated once and the fourth caller reads it rather than
  restating it.
  Three lengths ride along with no direction of their own — how far a
  mask's edge is softened over, how far a shadow is blurred, how wide an
  outline is drawn — and each is scaled by `Transform::max_scale`, which
  is the same figure the renderer already uses to turn them into device
  pixels, so the two agree. An uneven scale is not a thing a round
  softness can answer honestly and neither claims otherwise; the four
  page maps are all rigid, so nothing there was ever affected — this is
  a rule the new caller needed.
  Measuring the fix is what found the second defect, in the renderer: a
  shadow's offset was carried into device pixels by *scaling* it
  (`dx * scale`) rather than by putting it through the parent transform,
  so a group laid on its side went on casting its shadow down and to the
  right. `Effect`'s own doc comment said the shadow turns with the group
  it is in, and `map_page` already turned a layer's offsets when the
  page turned, on the same understanding that the light belongs to the
  page — the drawing code was the one place that disagreed, and no test
  had ever put a shadow inside a turned group.
  `a_dissolved_group_brings_its_children_masks_with_it` puts a softened
  mask, a shadow and an outline on each of the fixture's ten kinds,
  wraps each in a group that is then shifted, turned and scaled, and
  holds the page against itself across the dissolve; each of the six
  parts of the fix was checked by breaking it and watching that test
  fail. `a_shadow_turns_with_the_group_it_is_in` pins the renderer's
  half on its own.
- **And a layer's mask handed back out picks out what it let through:**
  handing a region to a layer and handing that layer's mask back out are
  the same carry read in the two directions, and the way in was already
  pinned. The way out is where the second copy of the carry lived —
  `Session::carried_into` wrote out, a second time, what
  `Mask::carried_through` says, so it still had the gaps that one has since
  had fixed. A brushed coverage's radii and the region a stroke was
  confined to went through untouched: a piece rubbed out of a layer sitting
  inside a group scaled by two came back out as a region with the brush
  still the size it was in there. It delegates now, so the rule is written
  once and the third caller reads it.
  Asking it as a *round trip* would have found nothing, and that is the
  lesson worth keeping: out and back again leaves an untouched radius
  untouched twice, and the two mistakes cancel exactly. The question has to
  be one-directional — the mask is handed out and given to a plain
  full-page layer at the root, and what the two layers show has to coincide,
  which is the whole meaning of handing a mask out as a region.
- **A region confines a brush on a mask too:** rubbing a piece out of
  anything but a paint layer goes into that layer's *mask* rather than
  over its pixels — which is what makes it something to change one's mind
  about — and a mask is brushed with the same tool and the same
  `PaintStroke` a layer is, so it is confined the same way. The engine had
  always written the region onto the stroke. The drawing code read it in
  one of the two places: `stroke.clip` was applied where a paint layer's
  strokes are drawn and not in `paint_plane`, where a mask's are, so an
  eraser used inside a region took the piece out of the whole layer. The
  test that pinned the layer half had a hole in it of exactly the same
  shape, which is why nothing said so; `a_brush_on_a_mask_stays_inside_
  the_region_too` is that test's other half, and block 9ay drives it
  through the tool.
  Carrying such a stroke between spaces wants three things rather than
  one, and `Mask::carried_through` now does all three: the points go
  through the transform, the radii are lengths in that space and go by its
  scale, and the region the stroke was confined to is another coverage
  over the same space and goes the same way. Each was checked by breaking
  it — and the first two attempts at the check did not bite at all, since
  a stroke laid down where its layer does not draw proves nothing about a
  mask.
- **And the same rule where a person meets it daily:** `reparent` — a
  layer dragged onto another row in the layers panel — already undid the
  change of space on the layer's own transform, so the layer does not
  jump when it is dropped into a group that sits away from the origin,
  and left the mask and the shadow behind in the space it came from. The
  layer stood still while the hole in it moved. The carry is the same one
  transform (`back ∘ was`) the code already had in hand, handed to
  `carried_through` as well as to the layer.
  Its own audit is narrower than the group one on purpose: reparenting
  changes the stack as well as the space — dropped somewhere else a layer
  is drawn in a different order, and a copy of the group it left holds
  something different now — so the claim is about the layer rather than
  about the page, and `Showing::Alone` is what asks it. Both ways round,
  through a host that turns and scales as well as shifts, and block 9ax
  drives the drag through the panel with a masked layer and probes across
  the mask's edge.
  Where the carry is *not* wanted is now a line worth stating: it is
  needed exactly when a layer's parent changes, since that is when the
  space its mask is written in changes. Aligning, flipping and a frame
  resizing its pinned children all keep the parent, so leaving the mask
  is the documented thing a layer does — moving behind its own mask.
- **Giving a layer its own adjustment leaves the page where it was:** the
  machinery is a group — the layer and the new adjustment go in one
  together, and a group holding something that reads the backdrop is drawn
  on a surface of its own, which is exactly what confines the adjustment
  to that layer. The other half of that is what nobody had looked at: a
  wrapped layer meets the group's surface rather than the page. A
  multiplying layer began multiplying against nothing and arrived over the
  page plainly, and a layer confined to the one below it was let out of it
  altogether and covered what it had been showing through — one press,
  from a menu that promises only to scope an adjustment.
  Both belong to the wrapper now, which sits exactly where the layer sat;
  inside it the layer paints plainly, which is also what the adjustment
  above it wants to read. Opacity is the one that stays: source-over with
  a weight is associative, so it comes out the same either way, and that
  is pinned rather than assumed. Asked with an adjustment that does
  nothing, so the claim is the plain one — the page before and the page
  after are the same page.
- **Adding an anchor to a path leaves the path where it was:** that is
  the whole of what adding one is for — somewhere new to take hold of a
  curve that is already the curve somebody wants — and the arithmetic is a
  de Casteljau split, which is exact on a path that has handles. A
  *smooth* path has none: its curve is a Catmull-Rom spline read straight
  off the anchors, which is what lets a path be drawn by clicking and what
  a brush stroke lands as. The split read those absent handles as zeroes,
  cut the segment as though it were straight, and gave the path authored
  handles — which win over `smooth` — so every other bend went too. Draw a
  line with the brush, double-click it to add an anchor, and the line was a
  polyline.
  The handles are derived first now, and the conversion is stated once as
  `chitrakar_render::smooth_handles`: a Catmull-Rom segment *is* a Bezier,
  `C1 = P1 + (P2-P0)/6` and `C2 = P2 - (P3-P1)/6`, and both handles of an
  anchor are the same vector negated, which is what makes the join smooth
  and why the path then behaves like one drawn with handles. What is left
  is a fraction of an alpha step at the edge, since the same curve cut into
  more, shorter pieces lands its antialiasing a hair differently; nothing
  moves by as much as a quarter of a step anywhere, over five kinds of path
  and four places to insert. Block 9ba drives the brush-then-double-click
  path and reads the ink the line lays down: a curve cut into chords is a
  shorter line, so that one number says whether the shape survived without
  guessing where the smoothed curve passes.
- **Combining shapes keeps the shape that was being combined:** the
  result takes the bottom-most operand's fill and stroke, because that is
  the shape the eye reads as the one being worked on — and by the same
  reading it *is* that layer with a different outline. It was built as a
  fresh layer with the fill and stroke put back into it, so a
  half-transparent shape with a drop shadow came out of a combine opaque
  and flat, its mask gone, unlocked, and let out of whatever it was
  clipped to. It is built from a copy of that layer now, which is also
  what makes the next field a `Node` is given come along without anybody
  remembering this code — the same lesson as `emit_copy` beside
  `emit_clip`.
  The test's sharp end needs no list of fields at all: the union of a
  shape and a shape *inside* it is the first shape, so the page must not
  change, and everything the layer says about how it is drawn is in that
  one comparison. The lock and the pin cannot show on a page, so those are
  said field by field beside it. Block 9az drives it through the panel.
- **A pass back over this stretch's own fixes**, which turned up two
  things and corrected one claim. A copy of a *copy* of something with an
  effect was not recognized as reaching out, so its shadow would be
  clipped again: making a copy of a copy collapses it to a copy of the
  original, but a file can say otherwise and so can a `SetKind`, so the
  chain is followed now — bounded, so a document claiming a chain nothing
  checked cannot spend the stack on it. Defensive rather than demonstrated:
  the case is reachable but the probe for it could not tell the two copies
  apart on one page.
  And `smooth_handles` was given a guard against a path too short to bend,
  on the reasoning that the arithmetic would reach for the anchor before
  the first — a clamp whose ends cross, which panics. It would not: the
  loop does not run at all on an empty path, and on a path of one that
  anchor is itself. The guard is gone again and the test that was written
  for it stays, with what it actually pins written on it, because clamping
  at the ends is exactly what a rewrite gets wrong.
- **Known wrinkle, found and left for a decision — what a clipped layer
  is confined to:** a layer held to the one below it is confined to that
  layer's *picture*, without the reach of its effects: the cover is taken
  from the base's own staged surface before its shadow and its outline are
  drawn. A *group* used as the base is confined to its composite, which
  does include its children's effects, because that is what the group's
  picture is. So wrapping a clip base in a group — which changes nothing
  else about a page — changes what is held to it, by four fifths of a
  step where the base's outline lies. Reproduced: a base with a drop
  shadow or an outline, one layer clipped to it, wrapped in a group of its
  own.
  Both readings are defensible on their own (Photoshop clips to the base's
  transparency and draws the base's styles over the whole run; a group's
  silhouette plainly includes what its children cast) and they are not
  consistent with each other. Making them agree is a choice about what
  clipping *means* rather than a fix, and it is not free either way:
  including effects means the cover has to be taken after the effect
  passes, which currently blend into the page rather than into the layer's
  own surface; excluding them means a group would have to be drawn twice.
  Left as it is, deliberately, and written down here so the next person to
  look at clipping starts from the reproduction rather than from the
  symptom.
  It was found by making the shared fixture's top shape *clipped* — which
  is not in the fixture, and for this reason: two audits' claims stop
  holding once a clip base is in the document, and one of them
  ("invisible is the same as not there") stops holding for a good reason
  of its own, since hiding a clip base takes what is held to it along
  while deleting the base lets that layer out. Accommodating both to gain
  clipping coverage everywhere else was the trade, and it was not worth
  weakening them for.
- **A copy of a layer casts the same shadow the layer does:** found by
  giving the shared fixture a drop shadow, which is worth recording as a
  method — the fixture had one of every node *kind* and no effect on any of
  them, so every audit built on it had been asking its question of layers
  that never draw outside their own box. One added shadow and two audits
  failed at once.
  A copy draws what the original draws, where the copy is, and the
  renderer decides where it may paint from the box of what it copies —
  which is the box the *handles* are drawn round, deliberately without the
  reach of anything's effects. So a copy of a group whose child casts a
  shadow had that shadow cut off at the group's contents: a shadow on the
  original, none on the copy, from a layer that is meant to be the same
  layer. Both the drawing extent and the dirty box needed it (the second
  showed up as wrapping such a copy in a group clipping it again), and a
  copy of something that reaches out now gets the whole page to paint in —
  the answer a copy of an adjustment or a filter already got. How far,
  exactly, would mean carrying reaches written in three spaces through the
  one a copy draws in; a copy of a subtree with effects in it is rare
  enough that the page is a cheaper answer than a wrong one.
  The GPU audit declines a document with an effect in it, so it clears the
  shadow the way it already removes the two kinds the backend cannot draw
  — and its "declined everything" guard is what said so, loudly, rather
  than the audit quietly measuring nothing.
- **A region render, padded by the reach, is the page:** everything about
  showing a document quickly rests on one claim — that a rectangle of the
  page can be recomputed on its own and come out the same as if the whole
  page had been drawn. It is not free: a blur reads its neighbours, a
  pixelate block is the average of what it covers, a clone reads from
  somewhere else entirely. So the caller widens the rectangle by
  `filter_reach` and keeps the interior, which is what the engine does
  before every repaint. What was tested was the *figure* — that the reach
  grows when the space a filter sits in does — and not the promise.
  `a_region_render_padded_by_the_reach_is_the_page` draws the page whole
  and then recomputes it over nine awkward rectangles apiece: a single
  pixel, a strip along each edge, a corner, a column, a block in the
  middle, and the whole page. Over the shared fixture, which has a blur
  and a clone layer in it, and then once per filter kind over a page with
  three overlapping blocks to read. Understating the blur's reach or
  dropping the pixelate block's fails it at the single pixel in the middle,
  which is the pixel a stale halo is hardest to notice around.
- **A layer that cannot be seen draws the same page as no layer at all:**
  there are three ways for a layer to be invisible and they are three
  different pieces of arithmetic — hidden is a flag the walk skips on, an
  opacity of zero is a weight at the end of it, a mask that covers nothing
  is a coverage read per pixel. For a layer that *covers* all three come
  to the same thing; for the four that draw by reading what is under them
  they do not, since an adjustment's opacity is how far to take the
  adjustment and its mask is where, so both are folded into the work
  rather than applied to a composite, and "none of it, nowhere" is a
  different line of code from "skip this layer".
  `a_layer_that_cannot_be_seen_is_the_same_as_no_layer` makes each of the
  fixture's ten kinds invisible each of the three ways and holds the page
  against the same document with that layer *deleted* — the strong claim,
  that the page is what it would be if the layer had never been added. And
  it first checks that the layer makes a visible difference at all, so a
  kind that draws nothing anyway cannot pass every question without
  answering one; that guard is the only reason this counts as an audit
  rather than a formality.
- **The blend modes that have a colour they do nothing to, do nothing to
  it:** nine of the sixteen have an exact neutral — white multiplied,
  darkened or burnt in; black screened, lightened, dodged, differenced or
  excluded; a middle grey in hard or soft light — and they are the
  sharpest test these formulas have. The neutral is where a mode's two
  branches meet (hard light is a multiply below half and a screen above
  it, so half is the seam), and the grey has to be a middle grey *in the
  encoding the blend is stated in*: these are read on display-encoded
  values, the way every specification writes them, so it is sRGB 0.5 and
  not the linear 0.5 that shows as 188. Reading it wrong shifts the
  picture by four fifths, which is what the check reports when asked with
  the linear figure instead.
  `a_blend_mode_leaves_the_colour_it_is_neutral_over` holds all ten to a
  millionth over a backdrop with colour and every tone in it, and asks
  each mode the same question at a colour that is *not* its neutral, so a
  mode that has quietly stopped doing anything cannot pass by doing
  nothing. The other seven have no fixed neutral and say so by name rather
  than being left out quietly; all sixteen are asked the one every mode
  owes, that a layer with no alpha leaves the backdrop exactly.
- **Every adjustment and every filter, set to do nothing, does nothing:**
  each of these is arithmetic on a colour and most of them go somewhere
  and come back on the way — into the display encoding because that is
  where a tone decision is read, into HSL because that is where a hue is,
  out to a channel mix and back. A neutral setting is the one input where
  the whole journey has to cancel and the one nobody looks at, since a
  slider is tried by moving it; drift there sits in the middle of a
  picture nothing has been asked of, and shows up as a photograph that
  changed the moment somebody added a layer and touched nothing.
  `nothing_asked_of_an_adjustment_changes_nothing` asks it of all
  twenty-one neutral settings there are, over a picture that sweeps hue
  and brightness so every band of a selective adjustment, both ends of a
  tone range, and the greys a vibrance is meant to leave alone all have
  pixels. Every one of them cancels to within a ten-millionth, and that
  is what the test allows — a tolerance written in eighth-steps of 8-bit
  grey would have let a drift of half a percent through, since a step near
  white is worth far more light than one near black, and the first version
  of this test did exactly that.
  Two of the thirteen adjustments have no neutral at all and are named as
  such: black and white always makes grey and there is no mix that does
  not. A gradient map looked like the third and is not — it replaces every
  tone by the colour at that tone's place along a ramp, so a colour comes
  back grey however plain the ramp is, which is the adjustment rather than
  a fault in it. Over *greys*, a ramp from black to white is the identity,
  and only if where a tone sits along the ramp and what the ramp says
  there are read in the same encoding: it gets a grey ramp of its own to
  be asked over.
- **A batch that fails leaves the document exactly as it was:** that is
  what makes a batch worth having — a gesture, a group, a layer added and
  masked in one breath are each several commands that have to land
  together or not at all — and the not-at-all half was tested once, with
  one failing command and a node count for a witness, which would not
  notice a page left a little turned or a counter that had moved on.
  `a_batch_that_fails_leaves_the_document_alone` puts every command the
  fixture knows in front of one that cannot work, both beside it and
  inside a batch of its own, and holds the whole serialized document
  against what it was. It found two things.
  A failed batch kept the ids it had taken: `next_id` had moved on, so
  "nothing changed" was true of everything except a counter. The ids are
  given back now — which is *not* the same question as undo, where they
  must stay taken, since an undone add can be redone and whatever referred
  to that layer still says its id.
  And multiplying a transform by a quarter turn or a mirror gives negative
  zeros where there were zeros, so a page turned right and then left came
  back to transforms *equal* to the ones it started with and written
  differently — a file where nothing had changed, in every layer of it,
  against a format that is careful everywhere else to save the same work
  as the same bytes. `Transform::compose` adds zero to each component now,
  a no-op on every value a transform can hold but that one, and
  `a_page_turned_round_and_back_saves_to_the_same_bytes` says so of a
  quarter each way, a half each way, and a mirror both ways.
  `StraightenCanvas` is the one command allowed to come back near rather
  than exactly, the same exception the inverse audit makes, and the slack
  is a unit — which is what a guide costs, since a guide is a line on an
  axis and carries no tilt, so it returns where it crosses the middle of
  the page. Nothing batches a straighten; the engine and the UI both send
  it alone.
- **And text with a styled run in it:** a run is a range of *bytes*, and
  what a run does not override is the block's own setting — so a block
  with one is the only thing that asks whether the two are read together,
  and the only text here drawn in more than one pass. Given its own colour
  and weight and an underline, so the difference is on the page rather
  than only in the file. The byte arithmetic behind editing runs has a
  property test of its own on the UI side (`app/e2e/runs.test.mjs`); this
  is the other half, which is a run being *drawn*.
- **And four more after that:** a *gradient* fill — a ramp baked from its
  stops and read across the shape's own box, which is a different path
  from a flat colour and is read at every pixel it covers; a *dashed*
  stroke *with markers* on each end, since a dash pattern walks an outline
  by length and a marker is a shape placed on a tangent; and a colour
  authored in **ink**, which on an RGB page resolves through the
  document's press profile or, without one, through the preview formula.
  The ink goes on the paint layer on purpose: a second renderer declines
  ink rather than guessing at a profile, and the paint layer is one the GPU
  audit removes anyway, so putting it there costs that audit nothing it
  was not already giving up. The paint layer keeps a second stroke in
  light beside it, so both ways of authoring a colour are on the page at
  once and neither branch can be right by accident.
  Everything held again. The forward-compatibility audit earned its
  both-directions assertion here: moving the paint stroke to ink left four
  entries in its list that nothing exercised any more, and it said so.
- **Four more things the shared fixture had never held:** a layer with a
  *blend mode*, which is the one flag that makes a plain group stop being
  transparent — a group holding something that reads what is under it is
  drawn on a surface of its own — so it puts every audit's question to the
  isolated path as well as to the straight one. A *group inside the frame*
  with a shape inside that, pinned to the far corner: an empty frame is a
  coloured rectangle and says nothing about being a frame, and nothing
  else here was nested two deep, so that one layer answers what a frame
  does to what it holds, what a group does inside another parent's space,
  and whether anything walking the tree stops a level short. And an
  *outline* and an *inner shadow*, so all three effect kinds are in the
  document rather than one — they are not variations on each other, being
  a ring outside the silhouette and a shadow kept inside it and painted
  over the layer rather than behind it, so each is a different pass and a
  different reach.
  Everything held, so this is coverage rather than a fix — but it is the
  coverage the last two finds came out of, and it costs nothing to keep.
  The GPU audit takes the effects off by walking the tree now instead of
  naming a layer, so the fixture can grow another without that going
  quiet, and it asserts that it found some to take off.
- **And the picture the clipboard gives back:** the same gap on the other
  side. The clipboard audit compared each arriving layer written out as
  text — its kind, its mask, its effects, how it composites — and then
  only that the document it arrived in *draws*. A layer can hold the same
  values and draw something else, and the case that matters is the plain
  one: a resource id names bytes that did not travel, so the picture
  arrives correct in every field and blank on the page. Each covering kind
  is now drawn on its own on both sides, with the nudge a paste gives it
  allowed for and nothing else. Stopping `paste` from restoring the pixels
  it carries fails it by a full step; taking a mask off, or moving one a
  pixel, is caught by the written-out comparison first, which is how the
  two divide the work.
  The four that draw by reading what is under them keep the written-out
  claim alone, since drawn on their own there is nothing there to adjust,
  to blur or to read from — that is said in the list rather than left as a
  gap.
- **And the page the file gives back, not only the account of it:** the
  file audit compares the document as a spelled-out account, and a
  resource appears there as *how many* bytes it has rather than as which.
  So pixels that came back changed — a picture written in a colour type
  that loses something, an alpha premultiplied on the way out and not on
  the way back — passed every line of it. The page is now compared as
  well, for each command, and a single eight-bit step in one channel fails
  it.
- **A mask read off an image, in the fixture:** the third mask kind was
  the one nothing in the shared document held. A shape's coverage is its
  own geometry and a brushed one is its strokes; this one has *pixels* — so
  it is the only mask whose coverage a renderer has to sample rather than
  solve, and the only reason a resource travels in a file other than a
  picture being on the page. It gets an image of its own rather than the
  fixture's picture, so a save has to carry a resource nothing on the page
  draws, which is the case that is easy to write a saver for and forget.
  The audits all held. It did find a weakness in the one above: the paths
  it names had numbers taken out of them, and a resource id is a content
  address rather than a number, so a resource's own fields were listed
  under the id they happened to have — a list that would need rewriting
  whenever a fixture's pixels changed. A segment is data now when it is an
  index *or* the key of one of the three objects keyed by data, which is
  the rule that was meant all along.
- **And layers cannot be nested past what a stack can walk:** a deep tree
  is a *legal* tree — no cycle, no layer named twice — and the check for
  those walks it with a stack of its own and is fine. Every walk that
  recurses is as deep as the tree, though: the renderer's compositing, the
  check for a copy of itself, the exporters. Ten thousand groups inside one
  another overflowed the stack, and the *editor* reached that before any
  file did, since the copy check runs after every structural edit and so
  building the nesting was enough.
  So there is a stated limit (`MAX_DEPTH`, two hundred and fifty six — far
  past anything a person nests, and Photoshop stops at ten), the way there
  is one on how large a page may be, and it is checked in both places: a
  command that would nest past it is refused, and so is a file that says it
  already has. Measured iteratively, because measuring has to be safe on a
  tree too deep to walk, and checked *before* the copy check, which
  recurses. The test also nests to exactly the limit and draws and saves
  the result, so the number is a limit rather than a wall a little before
  one.
- **A file that says its layers are not a tree used to crash on being
  opened:** a file names the layers there are and names what each group
  holds as two separate lists, and nothing about the format stops one of
  them naming a layer that is not in the other, naming the same layer
  twice, or naming one of its own ancestors. Every command in this editor
  keeps the layers a tree, so nothing that has been *applied* can be in
  that state — which is exactly why nothing looked.
  A group holding its own ancestor is a walk that never ends: such a file
  opened, and drawing it overflowed the stack and took the process with it.
  That is a crash from being handed a file, which is the worst way for "a
  file that says anything is refused rather than believed" to be untrue.
  `Document::check_structure` walks from the root and refuses a layer
  reached twice, a child that is not there, and a root that is not there,
  then asks the copy-cycle check — in that order, since that check walks
  the layers and would go round a cycle in them forever. Seven ways of
  saying it are tried, an `Instance` of itself among them.
  Refused where the counter beside it is repaired, and it is the same
  distinction: a counter is bookkeeping with a right answer to put in it,
  where a cycle is not something anybody meant and has no reading that
  keeps their work.
- **A file whose id counter is behind the ids in it used to eat a layer:**
  the counter is bookkeeping — nothing looks at it and it is only ever
  handed out — but it is written into the file with everything else, and a
  file saying a smaller number than the ids it holds is a file where the
  next `AddNode` takes an id that is already somebody's. The node under it
  is replaced and the tree is left with two places claiming the same
  layer: adding one layer to such a document silently ate another, or
  (depending on which id it landed on) failed with a cycle error about a
  copy of itself. A hand-edited file can say that, and so can one written
  by something that got it wrong.
  Put right on the way in rather than refused, and the difference from the
  hostile-file audit is the point: where a file's account of its artwork
  contradicts the artwork — a resource whose size does not match its bytes
  — there is nothing to do but refuse it; a counter is not the artwork, and
  throwing somebody's work away over a number nobody sees would be the
  wrong trade. `Document::settle_next_id` is the repair, and every number a
  file could say is tried, honest ones included, with the tree checked for
  agreeing with itself afterwards.
- **Every field a `.chitra` was ever given, taken back out again:** the
  one rule the format has is that an old file keeps opening — a new node
  kind or a new field is additive, written with `#[serde(default)]` so a
  manifest from before it existed reads as that default — and nothing
  checked it. A field added without the attribute makes every file
  anybody has saved unopenable, and the way that gets found out is
  somebody's work refusing to open.
  `a_file_written_before_a_field_existed_still_opens` saves the document
  of everything, then takes each of the manifest's three hundred-odd keys
  out on its own and asks whether the file still opens. The three dozen it
  cannot do without are named in the test: they are the fields the format
  has had since the beginning, and by the rule above that list is
  finished, so it is asserted in both directions — a field added without a
  default joins it and the test says which one by name. Then all the
  additive keys come out at once, which is as near as this can get to a
  manifest written before any of them existed: it opens, its layers are
  there, and the page draws.
  Two kinds of key are not one of these questions and are skipped. An
  enum's tag is a single-key object whose one key says which variant, so
  removing it makes a file that says nothing rather than an old one; and
  `nodes`, `children` and `resources` are keyed by data — a node id, a
  node id, a content address — so taking a key out of those is a file with
  a layer missing. Getting that second one wrong is what the first run
  found: every additive key removed at once had emptied the document, and
  the test said so.
- **Every command, over the boundary the UI talks across:** nothing in
  the app calls `apply`. The UI is TypeScript on the far side of a wasm
  boundary, so every mutation it makes is a serde-JSON `Command` handed
  to `Session::apply_json` — which makes the JSON shape of `Command` the
  editor's API, and a variant whose serde representation cannot make the
  round trip a feature that passes every native test and does nothing in
  the browser. One command had ever been sent that way in a test.
  `every_command_survives_the_boundary_the_ui_talks_over` applies each
  of the fixture's commands twice — directly to one document and, as its
  own JSON, to a copy — and holds the two documents and the two pages
  against each other, so a field written in a form that reads back short
  is caught rather than a field somebody thought to look at.
- **And the list of every command really is one:** the fixture has said
  since it was written that adding a `Command` without adding it there
  is what makes the audits fail, and that was true of nothing.
  `RestoreSubtree` — the command a delete undoes to, which carries a
  whole subtree rather than a value — was missing for as long as it had
  existed, so none of the five audits built on that list had ever asked
  its question about putting a deleted layer back. It is in the list now,
  as a removal and a restore *somewhere else*: put back where it came
  from it would change nothing, and the inverse audit rightly refuses a
  command that changes nothing, since then its inverse proves nothing.
  Keeping the promise is now the compiler's job. `fixture::variant_name`
  matches every variant with no arm for anything else, so a new one stops
  the file compiling on the line that says what to do; and
  `the_list_holds_every_command_there_is` holds the named list against
  what the list of things to do actually reaches, batches included. The
  five audits accepted the new variant as it stood, so this one found a
  hole rather than a bug — but it is the hole the other five were
  looking through.
- **A page of shapes, out through SVG and back in, is the same page:**
  five doors lead out of this editor and SVG is the only one it can also
  walk back through — `place_svg` brings a file in as editable layers — so
  the writer and the reader can be held against each other rather than
  each being trusted on its own. The audit below asks only that each door
  opens on whatever state a command left behind;
  `a_page_of_shapes_survives_being_written_as_svg_and_read_back` asks what
  came out the other side, and a shape whose fill-rule, handles or nested
  transform is written in a way this editor's own reader cannot make sense
  of is a file somebody opens in a year to find their drawing rearranged.
  Shapes rather than everything, deliberately: SVG carries no adjustment,
  no filter and no live raster, so those are lost by design and have
  nothing to be held to. What is asked is what SVG *does* carry — rects,
  ellipses, paths with handles, fills, strokes, opacity, groups with
  transforms of their own, and a hole read by the even-odd rule. The one
  thing that cannot come back exactly is colour, since a fill goes out as
  eight bits a channel; that is a twentieth of a step of linear light here
  and is what the tolerance is for. It came back right, and each half of
  the check was verified by breaking the writer.
- **And the two doors that can be read back are:** "it came out
  non-empty" is a long way from "it came out right" — a channel written in
  the wrong order, an alpha not composited, a picture at the wrong size are
  each a file of the proper length. So the PNG is decoded and held against
  the page to within one eight-bit step, in the encoding a PNG carries
  rather than in light; and the JPEG, which is lossy and has no alpha, is
  held to being the *same picture* over the page's opaque part, within a
  couple of steps a channel on average. Swapping red and blue on the way
  to a JPEG shows up as seventy steps; writing a PNG opaque shows up as a
  full one. SVG has a round trip of its own; a PDF and a CMYK TIFF are
  still only asked to open.
- **Every command, and then every door out:**
  `every_command_leaves_a_document_every_door_can_take` applies the
  shared fixture's every command and then asks for a PNG, a JPEG, an
  SVG, a PDF, a PDF of the frames and a `.chitra` — and opens the
  `.chitra` again. An exporter is where a node kind is forgotten: each
  writes the document into a form of its own, and a kind added a year
  later reaches all six of them without anybody thinking of it. Nothing
  in it says what the picture should *look* like — SVG and PDF have
  their own blocks for that — only that each door opens on whatever
  state a command left behind and gives back something rather than an
  error or a panic. Since the fixture holds one of every node kind,
  "whatever state" is a wide claim.
- **Every command is put through the file:**
  `every_command_survives_the_file` applies the shared fixture's every
  command, saves the document as a `.chitra`, loads it back and compares
  the two. A file is the only thing between one session and the next, so
  a field that does not survive it is work quietly lost — and lost
  silently, since nothing complains about a number that came back as its
  default. The comparison is a document *spelled out* rather than
  serialized: two documents compared as JSON agree about every field the
  JSON has and say nothing about a field it has lost, which is exactly
  the bug being looked for. `Debug` prints what is there, walked in the
  tree's own order since the nodes live in a hash map. Written per
  feature this is a check somebody has to remember to write; written
  over every command it is one a new `Command` runs into by itself.
  Spelling a document out means naming every accessor it has, and a new
  piece of document state has to be added there — the same rule the
  fixture keeps for commands. Anything left out is invisible to the
  audit, which is exactly as bad as not having written it: the regions
  a document keeps by name were added a chunk later and slipped through
  until they were named too.
  The fixture's two masks are softened on purpose — a softness is a
  number rather than a shape, which makes it the easiest thing in a mask
  to drop on the way through a file, an inverse or a change of space,
  and with both at zero every audit was blind to it.
- **Every command's inverse is checked by machine:**
  `every_command_undoes_to_exactly_where_it_started` builds a document
  with a group, shapes, a paint layer, a stroke and a guide, then walks
  one instance of *every* `Command` variant: apply it to a clone, apply
  the inverse it returned, and compare the two documents as JSON. It is
  the test that notices a new command whose inverse was written by eye,
  and it found one on the way in — mapping a guide through a page
  transform asked whether the two ends of the mapped guide shared an x,
  which is the same question for a quarter turn and a mirror and the
  wrong one for anything else: a page straightened by seven degrees
  turned every vertical guide horizontal, at a position that meant
  nothing. A guide is now carried as a line — a point and a direction —
  and the mapped direction picks the axis while the crossing of the new
  page's middle picks the position (`a_guide_keeps_the_axis_the_page_leaves_it_on`).
  `StraightenCanvas` is the one command the audit lets be inexact,
  because it re-fits the page to the turned artwork and cannot be undone
  to the bit.
- **Every command repaints what it changes:** the engine draws the page
  once and afterwards repaints only the region a command is computed to
  have dirtied. Getting that wrong does not fail loudly — it leaves a
  stripe of the last frame on screen, in the one place the user was
  looking. `every_command_repaints_every_pixel_it_changes` asks it of
  all of them the way a user would notice: draw the page, apply the
  command, and compare what the session repainted against the same
  document drawn from nothing; then undo and redo, since an inverse is
  a different command with a region of its own. The engine already
  keeps a couple of pixels of margin, so a region one or two pixels too
  small is absorbed; three is caught, and a region that forgot a whole
  node is far more than three. It asks the other half too: everything
  that actually moved has to lie inside the region the session *named*,
  since the app uploads that rectangle to the canvas and nothing else —
  a pixel that changed outside it stays on screen as it was, however
  correctly the engine drew it into its own buffer. It runs the whole
  list twice: once on the page at its own resolution, which is what an
  export does, and once through a zoomed, panned viewport that does not
  begin on a whole document pixel, which is what the app shows. The
  second pass found a real one: when the page changes size, the surface
  does not — a viewport is the window's size, not the page's — so
  "repaint the whole page" repainted the *new* page and left whatever
  the old, larger one had drawn beyond it standing on screen. A page
  that changes size now invalidates the whole surface. The app masks it
  today because every size change it offers re-fits the view, which
  invalidates everything anyway; the audit is what holds it. A third
  pass runs zoomed *out*, where a document pixel is smaller than a
  device one; nothing was wrong there, and the gesture audit's Escape
  check moved into a viewport too, with the same answer.
- **And the one thing drawn while it is looked at:** extending a brush
  stroke dirties only what changed between the old stroke and the new,
  since repainting the whole of a long one on every pointer sample is
  what would make it crawl. That is a bounds computation of its own,
  it runs on every sample anybody paints, and a region a pixel short
  does not show up later — it shows up as a gap in the line under the
  cursor. `a_brush_repaints_the_whole_stroke_it_is_drawing` draws one
  the way a hand does, turning back over itself with the radius
  swelling, and after every sample compares the repainted page against
  the same document drawn from nothing — on a layer and on a mask, with
  ink and with an eraser, through a viewport. Dropping one point from
  the changed box fails it. Both this and the
  inverse audit read one shared list — `chitrakar_doc::fixture` (feature
  `fixture`, a dev dependency) — so a new `Command` is added to it once
  and every audit starts asking about it. Its document is a group of two
  overlapping filled shapes, a paint stroke, a placed checkerboard
  turned and scaled, a block of text, a frame and a guide: the kinds
  whose pixels come from somewhere other than a shape's own geometry are
  the ones a question about pixels is most likely to be wrong about.
- **And every gesture, the same way:** every drag goes through
  preview/commit/cancel — the document updates on each pointer move so
  the user sees it, and history records one entry when the mouse comes
  up or none at all if Escape comes first.
  `every_gesture_commits_or_cancels_like_the_command_it_previews` asks
  both halves of every command in the fixture: committing lands exactly
  where applying it plainly would, with one entry in history rather than
  one per move; cancelling lands exactly where it started, with none,
  and leaves the *page* where it started too — a cancel that repaints
  too little is how Escape leaves a smear of an abandoned drag on
  screen. It also writes down the engine's actual contract, which was
  nowhere: only the *first* preview of a gesture keeps an inverse, so
  that one inverse has to undo everything the gesture goes on to do.
  Two shapes satisfy it and both are in use — every drag in the app
  restates the whole edit from the pre-gesture document (the full
  transform each move, never a delta), and the brush adds a stroke and
  then rewrites that same stroke, where removing it undoes the lot. The
  audit checks both: it repeats a command only where repeating it is a
  restatement, and it draws a brush stroke the way the brush does.
- **A group and something inside it, both picked:** the panel lists
  both, so it is an easy selection to end up with, and every operation
  that treats a selection as one thing was acting on the inner layer
  twice — an arrow key moved it two pixels for every one asked, and a
  flip flipped it back to where it started. A layer inside a picked
  group is already travelling with the group, so moving, flipping and
  aligning now leave it out (`Session::without_nested`, and the same
  question the panel already asked before deleting or duplicating).
  What is picked is also visible on the canvas now: every picked layer
  is outlined, the extras dashed and at two-thirds strength, with the
  resize handles moving to the box round the lot: several layers scale
  together about it, which was the one thing a multi-selection could not
  do — it could already be moved, aligned, flipped, ordered and deleted
  as a set. The scale is stated once in the document and put back into
  each layer's own space (`P⁻¹ · S · P · T0`), so a layer that is turned,
  or sits inside a group that is, comes in with the box rather than
  flying off. The rotation knob stays off a shared box: it would turn the
  one layer the panel is showing and leave the rest where they were.
  Three layers picked used to draw one box, so nothing on the artwork
  said what a drag or a Delete was about to take.
- **One layer aligns to what it sits in:** lining a layer up with the
  others it is picked with is what two or more mean; one on its own has
  no others, and "centre this on the page" — the alignment most often
  asked for — was an error saying it needed at least two layers. One
  picked now lines up with the frame it is inside, or the page when it
  is in none, and the buttons say which. Spacing evenly still wants two,
  since it is a statement about the gaps between layers.
- **A failing browser test says where and what:** the suite is one long
  script whose blocks build on the document the last one left, so a
  failure used to arrive as a line number and nothing else — and the
  interesting question is never the assertion, it is which block, and
  what the document had become by then. It now names the block, prints
  the layers and which of them were picked, saves a screenshot, and —
  the part that matters for the next run — closes the browser and the
  port, where before a failed run held 8123 and every run after it died
  on EADDRINUSE about nothing to do with the test.
- **Styling that stays on the letters it was put on:** a text block's
  runs are byte ranges, a textarea counts UTF-16, and every keystroke
  puts the whole list through `shiftRuns`. It now has unit tests of its
  own — `app/e2e/runs.test.mjs`, no browser, transpiled by the esbuild
  vite already brings — mostly a property over random strings and random
  edits with emoji and accents in the alphabet. That found the bug the
  function's own docstring says it exists to prevent: typing a word in
  front of a bold one made the new word bold as well. The two ends of a
  run answer differently where an edit lands exactly on one of them — a
  start is attached to the character after it, an end to the character
  before — and both were being moved by the same rule.
- **A region picked out of the page:** a selection, kept in the document
  as a [`Mask`] over the page — because that is what it is, and because
  that is what it is *for*. This editor is non-destructive: a selection
  here is not a stencil pixels are cut through, it is a region to hand
  to a layer, so "mask this layer with what I picked out" is the same
  value carried into another space rather than a conversion. `invert`
  gives the inverse selection for nothing; a region brushed by hand is a
  mask kind that already exists. It travels with the page — a quarter
  turn stands it on its end with everything else — and it is a
  `SetSelection` command like any other, so undo, redo and the audits
  all cover it without being told.
  Adding to, taking from and keeping the overlap of a selection are the
  *shape combinations the editor already had*: shift-dragging a second
  box round a selection is a union of outlines, and the same code does
  it. That arithmetic declines edges that overlap exactly rather than
  guessing, which is right for two shapes on the page and maddening for
  a marquee — a box dragged to take a bite out of a selection shares an
  edge whenever the drag starts on the same snap line — so on that one
  answer it asks again with the incoming region moved a five-hundredth
  of a pixel, an eighth of a per cent of the spacing between the samples
  a coverage is taken with.
  Four tools draw one, sharing a rail slot the way the shapes do: a box
  marquee, an ellipse, a lasso, and a wand under `M`. The box and the
  ellipse catch on the same lines a shape does — the page's own edge, a
  guide, the edge of the layer being cut around — at the start of the
  drag as well as the end, since a region laid against a layer's edge is
  begun from that edge. Ctrl says never mind and shift wins outright,
  the same two ways out a shape has, and for the same reason: shift is
  asking for a square, and one nudged onto a line would be neither
  square nor on it. The lasso is left out, having no corner to catch,
  and so is the wand, which drags nothing. A region *carried* across the
  page catches them by its own edges and middle rather than by the
  pointer, the way a layer being moved does: what wants to land on an
  edge is the region's edge, not the place inside it the hand happened
  to take hold of. The wand reads the
  page as it is drawn, spreads out from where it was clicked while the
  colour holds — judged on the values the screen shows, since a wand is
  a judgement about what *looks* the same and linear light does not
  agree with the eye about that — and hands the run of pixels to
  `chitrakar_render::trace_pixels`, which gives back an outline. Traced
  exactly, along the pixel boundaries themselves, because a staircase
  *is* the edge a set of pixels has and smoothing it would be inventing
  an edge nobody picked; runs that carry straight on collapse, so a flat
  region is a handful of points rather than four per pixel. From there
  it is a region like any other — added to, taken from, filled, handed
  to a layer — which is the whole reason for tracing rather than keeping
  a coverage. It asks either of two questions, and the rail says which:
  about the run of pixels the click landed in, or about the colour
  wherever it is on the page. A sky showing between branches is one
  colour in a hundred pieces and no amount of spreading walks from one
  piece to the next, so the second reading takes every pixel that looks
  like it and the tracer gives back that many rings — one region all the
  same, to be grown, softened, filled or handed to a layer like any
  other. Where two pixels touch only at a corner, four edges meet
  there and the walk turns as far clockwise as it can: that keeps the
  inside on its right the whole way round, and makes them two rings
  rather than one pinched figure of eight. Taking whichever edge came to
  hand made that a coin toss that landed differently from run to run.
- **A mask's edge, softened:** `Mask::feather`, the one thing an edge can
  be asked for that its shape cannot say — a region picked out of a
  photograph almost never wants the edge the marquee drew, and a layer
  masked into another wants to be let into it rather than stamped on it.
  A softened edge is a neighbourhood, so it cannot be answered a pixel
  at a time: the coverage is worked out into a plane over more than is
  being drawn (without the margin a dirty rectangle's own edge would
  fade and show as a seam), blurred by the same box radius the picture
  blur uses so a feather and a blur filter of the same sigma soften
  alike, and read from there. Inverting happens on the hard coverage the
  blur is worked from, which is the same answer either way round and one
  blur rather than two. Selections carry it too, and the number is the
  same one the mask panel shows — set before the region is handed to a
  layer rather than after. Shift while dragging adds
  to what is picked out and alt takes from it (both together keep the
  overlap), a click with neither lets go, a drag from *inside* what is
  picked moves the region rather than starting another one — whether a
  point is inside being the engine's answer rather than a test against
  the outline on screen, since an inverted region is everything *but*
  its rings, a softened one has an edge that is neither in nor out, and
  a painted one has no rings at all. An inverted region is a region like
  any other: its outline is the page's own rectangle with what was
  picked as a hole in it, so the ants run round both, and it can be
  added to, taken from, filled and cropped to — saying it had no outline
  made "pick out the rest instead" a dead end. Combining keeps the
  softness of what was already picked, since the second box says where
  rather than how sharply. And a region handed to a layer is carried
  into that layer's space whole — the shape through the transform, and
  the softness *as a distance*, which is easy to forget because it is a
  bare number rather than a point: a feather of four handed to a layer
  inside a group scaled by two faded over eight page pixels instead of
  four until the one place that carries a mask between spaces started
  scaling it too. And marching ants — a pale
  stroke under a dark dashed one, so the edge reads on a dark picture
  and a light one — say where it is. Where an outline cannot say
  enough, a wash can: the region's own coverage laid over the page,
  clear where it is picked and tinted where it is not. The ants draw an
  inverted region the same way round as an upright one, a softened edge
  not at all, and a region of a hundred rings as a thicket of them; the
  wash says all three at a glance, and being the coverage itself it
  cannot disagree with what the region will do to a layer. Offered
  beside the softness, with a region picked and a select tool in hand,
  and remade only when what is picked changes — which, while a marquee
  is being dragged, is every frame, so it is worked out at most a
  thousand pixels across and stretched over the page. It is an
  indicator rather than a mask, and that way it costs the same whatever
  size the page is. The ants are drawn from the
  outline the *engine* flattens, not one worked out again in the app: a
  rounded box, an ellipse and a freehand path each flatten differently,
  and two answers would show as ants off the edge. `Ctrl+A` means the
  page with a marquee in hand and every layer otherwise, since the tool
  in hand is the clearest signal of which of the two "select all" is
  meant; both are in the Edit menu either way, beside "pick out the rest
  instead" and the four things a region is good for. Mask this layer
  with what is picked, and the way back — a layer's mask, handed out
  into the page as a region. A mask is moved and resized on canvas but
  not reshaped there, and this is what reshaping one is: take it out,
  add to it, take from it, soften it, grow it, hand it back. The same
  carry as the way in, read the other way round, so the shape goes
  through the transforms and the softness through the scale. And its
  opposite — hide what is picked, which is
  what deleting a selection means done by holding the layer to
  everything *but* the region, so the layer is whole underneath and the
  region can be changed its mind about. Fill it, which turns the region
  into a shape layer of its own: a layer like any other afterwards, and
  for a lasso the only way to draw that shape at all (inverted, it fills
  the rest of the page — the page's own rectangle with the region as a
  hole, which is what an even-odd path means by a ring inside a ring).
  Filling a *softened* region is a shape with room round it rather than
  the region's own outline: a mask only ever takes coverage away, and
  the outward half of a fade lies outside the outline, so laid over its
  own shape a soft fill came out soft on the inside and sheer on the
  outside — the fade cut off at exactly the edge it was meant to cross.
  So it is a rectangle three sigma clear of the region on every side,
  clamped to the page, shaped entirely by the mask, and the shape and
  its softness land as one history entry.
  A region can also be taken *from* a layer rather than dragged out —
  the mirror of handing one to a layer, and the only way to pick out a
  shape no marquee can be dragged into: the words in a text block, a
  star, two boxes clear of each other as one region of two rings. What
  is picked is the shape the layer occupies rather than the picture it
  makes, so it is drawn on its own at full strength with its blend, its
  opacity and its effects set aside — a layer at two tenths still
  covers what it covers, and a drop shadow is not part of the drawing
  that casts it. Where the edge falls is half covered, which is where a
  shape's antialiased edge crosses the edge it is drawing — but half of
  what the layer manages at its strongest rather than half of opaque,
  since a shape painted in a colour that is itself part transparent
  would otherwise be said to cover nothing at all. An adjustment or a
  filter layer is asked differently, because it has no picture of its
  own: what it covers is what its mask lets through, and the whole page
  where it has no mask. Drawn alone it comes out empty, and "that layer
  covers nothing" is the wrong thing to say about a layer whose mask is
  often the most careful work on the page — the more so now that a
  region can be kept. Its mask is not set aside, since a masked layer covers
  exactly what the mask lets through, and a group answers for
  everything under it together.
  A region can also be taken further out, or further in, by a distance
  in page pixels — the matting move, since a wand pick carries a pixel
  of whatever was behind the thing it picked and shrinking by one loses
  that fringe. A distance and not a scaling: a long thin region grown
  by four gets four wider at both ends and along both sides. Measured
  on what is *covered* rather than on the outline, so it works the same
  on every kind of region — a rectangle, a painted one, an inverted one
  (where growing the region shrinks the hole) — and the answer comes
  back traced, the way the wand's does. The distance is a true
  Euclidean one, from the separable parabola-envelope transform of
  Felzenszwalb and Huttenlocher (`chitrakar_render::grown`): a chamfer
  approximation is square where this is round, and a staircase edge
  shows the moment the answer is traced back into a shape. Beyond the
  page counts as outside, so "select all, then shrink" insets from the
  page's own edge — and one cell of margin says that, whatever the
  distance: growing reads only cells inside the grid, and shrinking
  needs a single ring of outside, since the nearest outside cell to any
  interior point lies on the border. A margin as thick as the distance,
  which is what it asked for at first, made a grow of a million pixels
  an allocation nobody can serve, and a process that aborts is, in a
  browser, the editor going. A softness has the same shape of failure
  and now the same kind of answer: the box that averages a line is
  primed by walking the window once, so a radius wider than the line is
  clamped to it — a fade over the whole of a line is all there is to
  say past that. Both are reachable by typing a number and by opening a
  document that says one. The same reasoning covers the other numbers
  that arrive as numbers and leave as allocations: a viewport comes
  down by halves until it is a surface the machine can hold, the same
  rule a page is held to, and an export says no to a size larger than
  can be made rather than attempting it — a failed allocation ends the
  process rather than the request. Softness is left alone — how far a region reaches
  and how sharply it ends are separate questions.
  And crop the page to it, which is the resize the crop tool already
  does with the region standing in for the rectangle.
  A region can be *kept*, by name, and picked up again. A region is
  often the expensive thing on a page — a sky wanded out between
  branches, a lasso drawn round somebody's hair — and the only place to
  put one was the layer it was handed to. Kept regions live on the
  document beside the guides and the swatches, as the same `Mask` the
  selection is, so putting one away and taking it out again are a copy
  each way rather than a conversion that loses the softness or the
  inside-out. `SetRegions` is whole-list like the guides: keeping,
  renaming and forgetting are one command with one obvious inverse, and
  a name kept twice replaces rather than doubles. They sit as chips in
  the row the palette's colours sit in, taking the marquee's own
  modifiers — shift adds, alt takes away, both keep the overlap — since
  a region arriving from a chip is a region arriving; ctrl-click
  forgets one, which is the gesture the combining leaves free. They
  travel with the page like everything else written in its own space —
  the third thing `map_page`'s carry now takes along, and the one that
  most needed it: what is picked out is on screen and would be seen to
  be wrong after a quarter turn, where a kept region is not looked at
  again until the day it is picked up.
  With a region picked, the histogram is the *region's*: a histogram is
  read to decide where a picture's tones sit, and the picture in
  question is then the region. It is the same reading the levels and
  curves graphs are drawn over and the same one Auto sets its points
  from — one answer, so the graph and the button cannot come to
  disagree — and it is what makes grading one area of a photograph
  possible at all. Nothing about it is hidden: the region is on screen
  with ants round it. And the other half of the same wish: an
  adjustment or a filter added while a region is picked arrives
  *holding* it. The region is why the layer is being added, and the
  numbers being set are already the region's, so a layer that then
  covered the whole page would answer a question nobody asked. One
  entry in the history, since it is one wish — the id the add will use
  is known before it happens (`peek_next_id`), so the mask rides in the
  same batch and one undo takes both.
  A brush painted with a region picked out is confined to it, and stays
  confined after the region is let go of — which is what confining
  means, and why the region rides on the stroke (`PaintStroke::clip`)
  rather than being read off the document as it is drawn: a stroke held
  to whatever happens to be picked *now* would spill the instant the
  selection changed. Nothing is baked: the whole stroke is there under
  the clip, and taking the clip off gives it back.
- **The same document saves to the same bytes:** a `.chitra` is a
  manifest and one file per resource, and both came out in whatever
  order a hash map handed them over — a fresh order every run, since the
  seed is. Nothing read a file wrongly, but a document saved twice was
  two files holding the same work: nothing could compare them, and a
  version control system saw a change where there was none. The maps
  stay hash maps, which is what the lookups want; what goes on the page
  is sorted (`in_order`, and resources are a `BTreeMap` since the
  container writes a file per entry in that order). A hash map's order
  is stable within one run, so the test cannot catch the symptom by
  saving twice — it asks the thing that makes the symptom impossible.
- **Regions, in one breath:** a selection here is a `Mask` over the page
  — a region to *hand to a layer*, not a stencil pixels are cut through.
  Drawn with a box, an ellipse or a lasso; wanded, either by the run a
  click lands in or by that colour anywhere on the page; or taken from
  what a layer covers, which is the only way to pick out words or a
  star. Then: added to, taken from, inverted, softened, grown or shrunk
  by a true distance, kept by name and picked up again. And used:
  masked into a layer or hidden from it, filled as a shape, cropped to,
  copied out or exported as a picture in the shape it was picked in.
  And back the other way: a layer's mask comes out as a region, which
  is how a mask gets reshaped at all.
  Marching ants say where the edge is; a wash says which side is picked
  and how soft the edge is, which an outline cannot.
- **Next up (rough priority):**
  1. The GPU backend, in two halves. What is left to *teach* it:
     nothing of the node kinds: a brush layer was the last it had never
     drawn, and it draws one now
     (`a_brush_lays_the_strokes_the_cpu_lays`) — every stroke gathered
     into a coverage of its own with max blending, since the segments of
     one stroke union rather than pile up, then laid down in its colour
     or, for an eraser, taken off by a blend that subtracts it. What
     still goes back is a stroke confined to a region, which would be the
     mask machinery a second time over per stroke, and a clone layer,
     which paints with what the page already holds. Shadows it now draws,
     inner and outer, as the blur passes again read
     off the layer's own silhouette rather than off what is under it
     (`a_shadow_is_the_silhouette_the_cpu_casts`), on a leaf that is
     faded, masked or held to the one under it as well as on a plain one
     — all three decide what the silhouette is, so all three go into the
     surface the shadow is cast from rather than onto the quad that lays
     it down. What still goes back is a layer with a blend mode (the CPU
     brings the effect down by it too, and the one texture the shader has
     spare is already carrying the layer's coverage for an inner shadow),
     a layer with effects inside a frame (the CPU cuts the shadow at the
     frame's edge and not the silhouette it grew from), and a group. An
     outline stays the CPU's for a better reason: its band is a true
     distance, swept by a chamfer transform whose two passes each read
     what the one before wrote, and a parallel answer to that is a
     different band rather than the same one arrived at faster. The
     shared fixture still has its effects stripped before the audit
     compares, because two of the three layers carrying one are a blended
     layer and an outline. Both filters that used to be handed
     back it now draws: a motion blur as one pass along the line rather
     than the blur's six along the axes
     (`a_smear_runs_the_way_the_cpu_runs_it`), and a pixelate as two, one
     along each axis, since a block's average separates wherever the grid
     is upright on the page — which is the condition it is drawn under, a
     turned one going back because the blocks then lie at an angle and
     neither pass can walk them
     (`a_grid_of_squares_falls_where_the_cpu_puts_it`). Both work their
     geometry out where the CPU renderer works its own out and from the
     same numbers, which is what makes the two land on the same picture
     rather than on two plausible ones.
     What is left to *wire*: it now takes a view
     (`GpuRenderer::render_view`), which was the half that mattered — the
     surface has stopped being the page, so a viewport can be drawn from
     it, and the witness holds it against the CPU's own
     `render_region_at` at four views including one where the page is a
     patch in the middle of the surface
     (`a_view_draws_what_the_cpu_draws_into_the_same_surface`). Two
     things that were formalities while the two were the same rectangle
     are not any more, and both are now said rather than assumed: the
     page's own edge clips the artwork, and what reads a neighbourhood
     stops at the page's edge rather than at the surface's. Masks and
     clipped layers come along too: the coverage plane both renderers
     read is asked for the size of what is being drawn on rather than
     assuming the page's, which was a distinction that did not exist
     until a view did. What is left is for the app to reach for the
     backend at all, which is a question about where WebGPU is to be had
     rather than about this crate. The fixture
     audit (`whatever_the_gpu_agrees_to_draw_it_draws_the_way_the_cpu_
     does`) is what to run while doing either. See
     docs/spikes/gpu-rendering.md.
  2. Mobile shells: `tauri android init` / `ios init` (needs SDKs, so it
     wants a machine with Xcode/Android Studio).
  3. Depth. Two methods have been paying, and both are cheap enough to
     keep reaching for. One: **put something in the shared fixture that
     nothing there has ever held** — an effect, a blend mode, a mask read
     off an image, a group two deep — and see which audits stop holding.
     That found a copy drawing its shadow clipped, turned up the
     clipping wrinkle above, and — once a layer in it reached for a
     palette entry — found a palette change repainting nothing and the
     GPU declining a page it can draw. Three shapes have gone in since
     and every audit held: a copy of a *frame*, a copy of a *copy*, and a
     second frame. Holding is not nothing — but the copy of a copy was
     worth more than that. Nothing broke, so the question became what
     would have to break for the audit to notice, and the answer was
     nothing: stopping the walk that finds copies of copies after one
     round left every audit green. So that is now asked directly
     (`changing_a_layer_repaints_the_copy_of_the_copy_of_it`), and the
     lesson is the method's own — when the fixture gains something and
     everything holds, break the code the new thing was meant to exercise
     and see whether anything notices.
     Two: **ask an audit for the thing rather than an account of it** —
     the file audit compared a document written out as text and could not
     see wrong pixels; the clipboard audit compared a layer field by field
     and could not see a picture that arrived blank; the SVG witness read
     a dash pattern out of the markup rather than off the page, where a
     line on where it should be off would have passed. All three now
     compare the picture. What is left of the export audit is the doors
     that cannot be read back: a PDF has no reader here, so it is still
     only asked to open.
     Then whatever the next user of the editor misses first — a brush that
     paints pixels rather than laying down live strokes, and text shaping
     worth the name (`rustybuzz`/`parley`, weights, a face chosen per run
     rather than per block).
- **What the view shows of the document** is one setting
  (`chitrakar_render::Showing`) rather than a growing pile of flags: the
  page, one layer on its own, or the picture before the work. Both of
  the last two are questions about *looking* — nothing about the
  document changes, so nothing goes into the history or the file, and
  letting go gives back the same bytes rather than a redraw that
  happens to agree.
  **Before** leaves out every layer that only changes what is under it,
  which is the question a photograph asks every few minutes and which
  used to take hiding each adjustment by hand and undoing them all
  again — edits the file would remember, for a glance nobody meant to
  keep. It is done by skipping those layers exactly where a hidden one
  is skipped, so the answer to "before" cannot come out of a different
  arithmetic from the answer to "hidden"; a test holds the two pictures
  against each other. It is a question about every layer rather than
  about the top of the tree, so it travels down the walk, which is the
  one thing here that costs a parameter in the recursion.
- **One layer on its own:** alt on a layer's eye asks the other
  question the eye is about — not "is this one shown" but "what does
  this one look like by itself". The page keeps its own framing and
  edge with that layer drawn on it and nothing else, its effects and
  mask and all, since what is wanted is the layer as the page draws it
  rather than a stripped-down version. It is a setting of the *view*,
  like soft proofing: nothing about the document changes, so nothing
  goes into the history or the file, and the page comes back exactly as
  it was — the same bytes, not a redraw that happens to agree. A layer
  that goes takes the view with it, by an undo as readily as by a
  delete, since a view of nothing is not what anybody asked for.
- **Chrome:** "?" (or View › Keys and gestures) opens a sheet of every
  key and gesture, since half of what this editor can do is a gesture
  nobody would guess at — which is also why a gesture added without a
  line on the sheet leaves the sheet quietly wrong, and why the sheet's
  own block holds it to naming them and not only to its letters
  working. Document actions live in a File/Edit/Page/View menu bar — Edit
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
  picture (Edit › Copy as image puts a PNG on the system clipboard: a
  region picked out of the page when there is one, in the shape it was
  picked in rather than as the rectangle round that — which is the only
  way a lasso or a softened edge reaches another application at all —
  and otherwise the picked layers' box; File › Export selection as PNG
  goes by the same rule, being the same picture leaving by another
  door, and offers it at twice and three times as the page's own
  exports do — an asset picked out of a design is usually wanted at the
  size the screen it is for has pixels for, and the region's box stays
  the region's box while the picture in it is drawn bigger), and images from other applications come *in*, by paste or
  by dropping a file on the canvas (a dropped .chitra opens). A picture
  arriving in a document with nothing in it and nothing behind it is a
  picture being *opened*, and the page takes its size — a photograph
  opened on somebody else's page size is the wrong answer to that, and
  nothing is disturbed because there was nothing there, which is the
  condition actually asked rather than "the page looks empty". On a
  page that is already somebody's it is laid down whole and in the
  middle, taken down to fit if it is bigger: at its own size a
  photograph hangs off three sides and shows a corner of itself, which
  is not a picture anybody placed. A picture that took the page's size
  with it takes the *view* too, since the framing that was there was
  framed for a page that has gone — without that, a photograph opens
  showing a corner of itself, which is the thing taking the page's size
  was supposed to stop.
  Effects come in three kinds;
  and export flattens them like everything else; SVG export sets each
  line the renderer set — wrapped, aligned by text-anchor, on its real
  baseline, at the em the face is scaled to — though a synthesized
  italic lean and a loaded font are the reader's to supply; a mask is an ellipse, a rectangle, or
  another shape handed down to the layer below, moved and resized on
  canvas but not reshaped there — reshaping one goes the long way
  round, out into the page as a region and back; PDF export is live where PDF has the
  words (paths, solid fills and strokes, groups, images, opacity, blend,
  text in embedded faces, subset to the glyphs used) and the engine's
  pixels where it does not (gradients, effects, masks, varying strokes;
  an adjustment or filter flattens what is under it), and TIFF is the
  composite; a boolean
  operation flattens curves to line segments; outlines whose edges
  overlap exactly are declined by the arithmetic and asked again with
  one side moved a five-hundredth of a pixel
  (`boolean::combine_or_nudge`), since snapping is *for* landing edges
  on each other and two rectangles snapped edge to edge is the union
  people ask for most.

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
  out-of-gamut pixels mark neutral grey. Monitor profiles ✅: an sRGB →
  display transform at the same step, after proofing (proofing says what
  the press would make of the picture; this says what this screen makes
  of that), from a loaded .icc or the built-in Display P3. Rendering-
  intent selection is still pending, and blocked upstream: moxcms 0.9
  stores `TransformOptions::rendering_intent` but never reads it, and all
  four intents give byte-identical output through a real CMYK profile, so
  a picker for it would be a control that does nothing.
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
  A frame carries the multiple it wants exporting at (1×, 2×, 3×), so
  which screen it is for is part of the frame rather than something to
  remember at the moment of exporting ✅.
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
