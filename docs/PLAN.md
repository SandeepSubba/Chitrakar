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
  white balance, vibrance, posterize, threshold, levels,
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
  layers — paths with their curves, solid and gradient fills, strokes
  with their dashes, caps and joins, group opacity, text as outlines —
  in document space, one undo step. A picture the file carries comes in
  with them, in the place the file drew it, as a raster layer referring
  to pooled pixels; a nested `<svg>` comes in as more shapes. A path SVG
  fills by winding is converted rather than read as even-odd, which is
  the engine's only rule — exactly, where its rings are wound the same
  way, since nonzero is their union there. And what a `clip-path` hides
  stays hidden, a mask that is only a region with it: nesting
  intersects, several outlines union, and the region rides down to each
  shape as an ordinary vector mask.
  What is still left out, each for a stated reason: a `filter` other
  than a shadow, an outline or an inner shadow (those come in as the
  engine's own effects now, see "And a shadow" in §0's import notes), so
  a blurred element comes in sharp — there is no way yet to say "blur
  this one layer", a filter layer here reaching everything below it and
  a clip to the shape cutting the very spread that makes it a blur.
  That is a layer that arrives plainer than the file, never one that
  arrives missing. (A mask with real grey in it was on this list too,
  and comes in now.) (Two more were on this list: a
  pattern fill, which comes in now as a picture of its tile repeated
  and seen through the shape's outline, and `stroke-dashoffset`, which
  has a field now — see the later entries.)
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
- **At a phone's speed, October 2026.** The whole editor, Chromium
  throttled four times over, a run of wheel zooms on the production
  build (`tools/bench/app/gesture.mjs`, `THROTTLE=4`). A step includes
  the bench's own round trips — a page with nothing on it takes 98 ms a
  step — so read the rest against that floor. A page of forty text
  blocks: 249–286 ms a step before the encode, text and outline work
  below, 161–170 after. A page of sixty shapes with drop shadows:
  708–721 before the shadow and compositing work, 294–334 after. Where
  the engine's own frames went, by the WebAssembly engine alone, is in
  each entry near the end of this section; the next thing to look at on
  the shadows page is the coverage blur itself and the window surfaces
  each shadowed layer allocates. Tried and not taken: leaving a shadow
  unstamped under the pixels its own opaque layer will cover in Normal
  (exact, since the layer over anything is the layer). On that page the
  shape is about a third of each shadow's window, the rest being halo,
  and the gain was about 2% of a frame, inside the bench's noise — not
  worth a surface threaded through every effect call. And encoding a
  frame's bytes by rows as well as by runs (a row the same as the row
  before taking that row's bytes whole, the margins of a page being
  such rows): exact, and slower in WebAssembly, where comparing two
  rows byte by byte cost more than the pixels it saved — an empty
  page's frame went from 5 ms to 10 and the text page's from 22.5 to
  24.6. The encoder's test keeps the repeated rows it was given, so an
  encoder that tries this again has them to answer. And WebAssembly's
  128-bit lanes (`-C target-feature=+simd128` on the wasm target):
  measured through a WASI build of the engine's bench, the photograph
  went from 26.3–27.0 ms a frame to 24.3–24.7 — eight per cent, from
  whatever the compiler vectorises on its own. Not taken: a page built
  with them does not load at all in a browser without them (Safari
  before 16.4, 2023), and eight per cent is not worth either dropping
  those or shipping the engine twice and choosing at load. Worth more
  once a loop is written for the lanes rather than left to the compiler.
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
- **A page-sized surface costs nothing to make.** `vec![x; n]` asks the
  allocator for zeroed memory only when the element is a primitive the
  standard library knows about; a struct of four zero floats loses that
  and is written one element at a time. For an A4 at three hundred dots
  an inch that is a hundred and forty megabytes written before anything
  is drawn — and measured, it was the *whole* of what an empty page took
  to render. Zeroed memory instead (`chitrakar_color::transparent_run`),
  which is pages the system already knows are zero.
  On its own that trades one cost for another: the pixels a drawing does
  reach now fault in one page at a time wherever the painting wanders,
  where a single sweep takes them in order and the kernel hands over
  several at once. So the part about to be painted is swept first
  (`Surface::warm` over the drawing's own box), which for a page-filling
  drawing is all of it and for a small one almost none. Serially, three
  runs each: an empty A4 66ms → 0.04ms, a tiny layer on one 65ms →
  0.4ms, a tiny layer in a group 71ms → 0.4ms, one with a shadow 77ms →
  1.6ms, and a page-filling drawing ~301ms → ~274ms. That last one is
  the lesson as much as the number: a single reading had said 257ms and
  made the change look like a 22% regression, which three readings of
  each side turned into a 9% gain. Timing probes here get run more than
  once now.
  What makes it safe is that every field of `LinearRgba::TRANSPARENT` is
  a zero float, which is asked of the bits *and* of the type's size —
  a field added later would still leave those four zero while quietly
  filling every surface in the engine with something else.
- **A flat colour is the same colour everywhere, and a rectangle is
  spans.** Chasing the page-sized surface above turned up the next thing
  down: a page-filling rectangle on an A4 at 300dpi cost twenty-five
  nanoseconds a pixel, fifty-odd cycles to put one colour down. Two
  things, both of them work spent on answers that were already known.
  The row loop asked the paint what colour it is *at a point*, and worked
  the point out by inverting the transform — six multiplies and two adds
  a pixel for a `Paint::Solid` — and then asked which blend mode to use
  once per pixel off a value that cannot change inside a row. A flat
  colour under no mask is now its own loop over two slices, with Normal
  split out, which is a multiply and an add per channel and nothing else.
  And a rectangle standing square on the page had no scanline form at
  all, where an ellipse and a path both did — so it never reached that
  row. Its exact coverage is a product of two one-dimensional overlaps
  and neither factor changes as the other moves: the across is worked out
  once for the shape, the down is one number a row. The same arithmetic
  in the same order, so the picture is the same to the bit — which is
  what the cross-renderer audit and the pixel-exact cached-render tests
  say, and what `a_rect_covers_the_area_it_really_has` asks for directly
  (quantize that product the way a sampler would and it is the only thing
  that fails).
  A page-filling rect 217ms → 110ms, two of them 370ms → 125ms, a
  1800-pixel disc 85ms → 70ms, and the A4 probe the two of these were
  found through 301ms → ~135ms. A turned rectangle is unchanged and
  falls through to the sampler, since a product of overlaps is not its
  area.
- **A blend over an opaque backdrop is the blended value and nothing
  else.** The dearest thing this renderer does per pixel is a blend that
  is not Normal — a full-page Multiply on an A4 at 300dpi took 840ms —
  and most of a page is an opaque layer over an opaque one. There every
  step of the W3C compositing reduces: one over one is one, the two
  weights are zero, the third is one. Said out loud, Multiply went 85ns a
  pixel to 53ns and a full page 840ms to ~560ms; the four that read all
  three channels at once got the same reservation, Color 77ns to 70ns and
  a full page to ~610ms. The condition is written as *equality* rather
  than as nearly, because at exactly one every step of that reduction is
  exact in floating point — so it is the same bits as the general form
  and not a hair off it, and an alpha that has somehow come out above one
  goes the long way round as it always did.
  What guards the two paths against coming apart is that the fast one
  runs for every pixel anybody looks at, so a mistake in it would go
  unnoticed by anything comparing pictures. They are held against each
  other by walking up to opacity from just below: at an alpha of 0.999
  the general path is what runs, and its answer must be the opaque one to
  within the thousandth the alpha is off by, for every mode and every
  pair of colours (`an_opaque_blend_is_the_limit_of_a_nearly_opaque_one`
  — swap source and backdrop in the fast path alone and it is one of only
  two things that fail).
  One thing tried and thrown away first, which is the more useful half of
  the story: those nine table lookups a pixel are found behind a
  `OnceLock`, consulted once per pixel, and the obvious reading is that
  an opaque call in the hottest loop in the renderer stops the two table
  pointers being kept in registers. It was threaded through to be found
  once a row instead — and it bought two percent. The cost is the
  arithmetic itself, not finding the tables, so the parameter came back
  out again. Worth the hour to know which.
- **A picture at its own size is its own texels.** A survey of the layer
  kinds that had never been timed found the biggest number in the
  renderer and the commonest thing a photo editor does: a page-filling
  picture on an A4 at 300dpi, 400ms. A blit takes four texels and mixes
  them, which is right when a picture is being enlarged and pure waste
  when it is not — laid down at its own size, square to the page and on
  whole pixels, the sample lands on a texel's own centre and both
  interpolations are between a value and itself. It takes the one texel
  now: ~411ms → ~181ms, three readings each way. Bit-identical, since
  mixing `a` and `b` by nothing is `a + (b - a) * 0`.
  And the one division a pixel — the source byte's alpha over 255 — is a
  256-entry table beside the sRGB one, holding what the division gives
  entry for entry, so it is the same number rather than a near one
  (`v * (1.0 / 255.0)` is not, in the last place). Worth ~7% on a
  resampled blit, where there are four of them a pixel.
  What pins it is `a_picture_at_its_own_size_is_its_own_texels`, which
  holds every pixel of the page against the source byte it came from
  rather than taking a tolerance over the whole picture: shift the texel
  picked by one and it is the only thing in the workspace that fails,
  because a photograph of smooth things survives being moved by a pixel.
  Two things measured and thrown away with it. A rectangle's exact
  coverage is separable and a picture's outline is a rectangle, so the
  blit's edge coverage can be a row times a column the way a rect fill's
  is — it bought two percent and a branch in the hot loop, and went back
  out. And `x as usize` was replaced by `x.floor() as usize` in the
  transfer-curve lookup on the reasoning that flooring is one instruction
  where a cast is a conversion each way: it made a blend *twice* as slow,
  102ns against 52ns, because baseline x86-64 has no `roundss` and
  `f32::floor` is a call into libm. A cast is the fast one here.
- **Where the floor is, and four things that did not beat it.** The
  adjustment pass looked like the next thing to take: a full page of
  exposure 230ms, of hue/saturation 340ms, on tools a photo editor
  reaches for constantly. It is not, and the measurement that settles it
  is worth having written down. The same page with *no* adjustment on it
  is 88ms; with the cheapest adjustment there is — a white balance, three
  clamps and three multiplies, no call into libm and no matrix — it is
  221ms. So the pass itself costs 133ms whatever arithmetic it does, and
  133ms is exactly one read-and-write sweep of a hundred and forty
  megabytes: the same ~65ms that touching the page once costs, doubled.
  The adjustment pass is at this machine's memory bandwidth and there is
  nothing in it to win. What hue/saturation spends above the floor is a
  real 3×3 matrix.
  Four attempts went into finding that out, and every one of them failed
  in a way worth knowing:
  - *Hoisting the transfer tables* out of the per-pixel blend, on the
    reading that an opaque call through a `OnceLock` in the hottest loop
    stops two pointers being kept in registers. Two percent.
  - *`x.floor()` in place of `x as usize`* in the curve lookup, on the
    reading that flooring is one instruction where a cast is a conversion
    each way. Twice as slow — 102ns against 52ns — because baseline
    x86-64 has no `roundss` and `f32::floor` is a call into libm.
  - *Hoisting each adjustment's own constants* into what `prepare`
    already returns, since an exposure raised two to a power and a hue
    rotation took a sine and a cosine and built nine numbers, per pixel.
    Hue/saturation got **25% slower**: LLVM was already hoisting that
    work out of the loop, being pure and loop-invariant, and keeping the
    matrix in registers — where reading it from a struct behind a
    reference is a forty-byte load a pixel. The compiler had done it
    better than the hand.
  - *One reciprocal in place of three divides* to unpremultiply. No
    change; three divides of a vector are one instruction.
  Which is the lesson to carry into the next round of this: measure the
  floor first — a page with the thing taken out — and only then ask what
  the thing costs above it.
- **A shrunk picture was sampled twice over, and a probe for what a hand
  feels.** Every timing above renders a whole page from nothing, and the
  app repaints a window of a document a dirty rectangle at a time — so the
  lesson about measuring the floor was applied to the thing a user
  actually waits for. A drag of a small layer on an A4 with a photograph
  filling it: 0.9ms. One more sample of a brush stroke: 0.1ms. Dragging
  an *adjustment layer's opacity* over that page, zoomed out: **330ms**,
  three frames a second, and no full-page probe could have said so.
  It is not the adjustment. A window of the photograph at nine tenths of
  its size repainted in 203ms where the same window at its own size took
  22ms — slower for less picture — because the sample count was the
  *ceiling of the footprint*, which jumps to two the moment a picture
  shrinks at all: four bilinear taps and sixteen texel reads a pixel to
  average over 1.1 texels. A bilinear tap is already the average of the
  two texels either side of it, weighted to sum to one, so taps two
  texels apart cover the footprint with nothing missed between them. One
  every two texels, then: 203ms → 58ms at nine tenths, 162ms → 51ms at a
  half, 290ms → 91ms at a quarter, and the opacity drag 330ms → 160ms.
  The answer is the same answer, and that was checked rather than assumed:
  at half size one tap is exactly the average of each two texels by two —
  and so is the old count, which passes the same test
  (`a_picture_at_half_its_size_is_each_two_texels_averaged`). What its
  four taps bought was nothing. The count is bracketed from below by the
  test that was already there: hold the taps at one and
  `a_minified_raster_averages_the_texels_it_skips_over` fails, which is
  the crawl a shrunk photograph gets when it moves.
  `live_editing_probe` in `core/engine` keeps all of those numbers, since
  the drag was the only way to find this and nothing else measured one.
- **Asking who notices a sabotage?** Use `cargo test --workspace
  --no-fail-fast`. Without it cargo stops at the first test binary that
  fails, so a break in `core/render` hides whatever `core/gpu` would have
  said about it — which turned a count of six into a count of one, and put
  the wrong number in a commit message. The order is codecs, color, doc,
  render, gpu, engine, so a render failure hides gpu and engine and a
  codecs failure hides everything.
  Two things limit how far back that reaches, and both were checked
  rather than assumed. Truncation only happens once something *fails*, so
  every "nothing noticed" in the record below is sound — the run went to
  the end. And a count is a lower bound rather than a wrong number. Three
  of the counts that carried an inference were re-run with
  `--no-fail-fast`: the selection carry is three and the kept-region carry
  is two, exactly as written; the stand-in walk reads seven now against
  the two recorded, and five of those seven are tests that sabotage
  caused to be written. Its inference — that nothing outside the renderer
  had ever looked at a copy's stand-ins — holds, since nothing in gpu or
  engine fails even now.
- **Desktop shell, files by path:** in the shell a document has a file.
  Save writes back over the one it was opened from or last saved to and
  asks only the first time; Save as (Ctrl+Shift+S, shell only — a
  browser's every save is already a new file) asks again and carries on
  there; Open and Ctrl+O use the system's panel; an export asks where,
  beside the document, and an export of several files (a set, slices,
  every artboard) asks for a folder once rather than a panel per file;
  File › Open recent lists the last ten by name (a folder beside the
  name only when two share one), newest first, and a file that has gone
  says so and comes off the list. The list is `prefs.recent` — about
  the person and the machine, not the file. A browser is unchanged:
  `app/src/files.ts` is the one place that knows which it is in. The
  commands are the shell's own (`choose_to_open`, `choose_to_save`,
  `choose_folder`, `read_path`, `write_path`, `join_path` in
  `shells/tauri/src-tauri/src/lib.rs`): bytes cross as bytes and a
  write's path beside them in a header, percent-encoded since a header
  is ASCII, and a write lands whole or not at all — beside the file
  first, renamed over it once it is all down (`files::write_whole`), so
  a full disk halfway through a save leaves the old document rather
  than half of each. Panels run in async commands, since a blocking
  panel on the thread that draws the window deadlocks it. The shell
  crate builds and tests here once `libwebkit2gtk-4.1-dev
  libsoup-3.0-dev libjavascriptcoregtk-4.1-dev librsvg2-dev` are
  installed (`cd shells/tauri/src-tauri && cargo test`: path decoding,
  extensions, the whole write); the browser suite stands the shell in
  with a bridge whose filesystem is a Map and whose panels answer from a
  queue (block 9ca), which is all the app ever sees of a shell anyway.
  Files dropped on the window come by path too: the shell takes a drop
  of files itself (the page never sees one the browser's way, so before
  this a photograph dropped on the desktop window simply did not
  arrive), a picture is read and placed, and a document opens with its
  file, so Save writes back to it. Closing the window asks about unsaved
  work — a window closed from its title bar never fires `beforeunload`,
  so the close box used to throw work away without a word — and the
  title bar says the document's name, with a • while there is work to
  save. Opening by dropping asks about unsaved work in a browser too,
  which it never had. The shell's capability grants `core:window:allow-
  destroy` and `allow-set-title` for these; the stand-in bridge
  delivers events the shell's way (a callback registered by number) and
  block 9ca closes the window and drops files through it.
  The draft keeps the file too, and whether the file already held
  what the draft holds (`name`, `path`, `saved` beside the bytes):
  restored, a document saves back where it was, and comes back as work
  still to save unless it had been saved — before, a restored draft
  came back looking saved, in the browser as well, so the work it
  rescued could be lost a second time without a question. An export of
  several files into a folder asks before writing over any already
  there, once for all of them (`already_there`), since there is no
  save panel to ask.
  A document double-clicked opens: the bundle declares `.chitra` as the
  app's own (`fileAssociations`, an exported type on macOS) and PNG,
  JPEG and SVG as files it can also open, and the shell gathers what it
  is asked to open — the command line on Windows and Linux
  (`opening::files_in_args`, made whole against where the app started),
  `RunEvent::Opened` on macOS, which comes after launch and again for
  every file opened while it runs. They wait in a queue
  (`opening::Waiting`) until the page asks (`opened_files`), at start
  and on each `chitrakar://opened` nudge, so a file that arrives before
  the page is there is opened all the same, and each only once. A
  document opens with its file and a picture is placed, as a drop. The
  macOS branch cannot compile on Linux at all — not even a `cargo check
  --target x86_64-apple-darwin`, since a dependency's build script
  compiles Objective-C against Apple's SDK — so CI runs the shell's
  clippy on a macOS runner too (`shell-macos`), which covers the Swift
  subject matte's build as well.
  A file double-clicked while the app runs reaches the copy already
  running on Windows and Linux too: the single-instance plugin hands
  the second launch's command line to the first (`second_copy`), which
  brings its window forward and queues the files as above — without it,
  every double-click started another copy of the app.
- **A file that says an extreme number is drawn or refused.** The page's
  and every picture's sizes were already checked on the way in; what a
  *layer* says about itself was believed and drawn. A probe set numbers
  in the fixture's pages to extremes (±10³⁰, ±10⁹, 2³², 10⁻³⁰, −1, 0),
  each in a child process with a deadline so that an abort or a hang is
  a finding rather than the end of the run, and opened, drew and wrote
  each as SVG and PDF: sixteen of the first four hundred and eighty took
  the program down. A text block a billion pixels tall or set a billion
  apart was floored back up to a fiftieth of its size and asked for a
  raster no machine has (the ceiling now has the last word, a raster
  past 8192² is nothing drawn, and no letter is drawn taller than 2048
  pixels); a synthesized bold was smeared in steps of a third of a
  pixel however large the letter, so a bold headline zoomed in was
  hundreds of passes a glyph and a huge one more than memory could count
  (at most sixty-four now, each a thousandth of the letter at most); a
  clone layer shown on its own, in a group scaled past any screen or
  lifting from a billion pixels away, asked for a page under it that
  size (cut to the page, which is all there is to lift from); and a
  dashed stroke a negative width wide walked a ring so long that a step
  along it did not move (a stroke of no width lays nothing, and past a
  hundred thousand dashes a line is drawn solid). Then 3,600 more: none
  fatal, four slow (three seconds for a layer scaled by billions drawn
  into a PDF's effect picture), left. Kept as
  `a_layer_that_says_an_extreme_number_is_drawn_or_refused` — a
  catalogue of the fields that broke, over eight pages, in a minute and
  a half of a debug build — and
  `a_dashed_stroke_past_reason_is_drawn_or_dropped_not_walked`, since
  none of those eight pages has the dashed inside stroke the hang
  needed. The same guards protect what an SVG placed in a document can
  say, which goes through the same renderer.
- **And an SVG that says one.** The same probe aimed at placing an SVG —
  this editor's own export of the fixture's pages, one number at a time
  set to an extreme — stopped the program on twenty of four hundred and
  eighty. Three were this importer's: a shadow's blur found by counting
  up from one (a step for every pixel of its width; a closed form now),
  a soft mask held to a number of pixels but not to a side (one a
  trillion wide and two tall asked for a row no machine has), and the
  raster drawn under that mask. The rest were the reader's own — text
  along a curve with a point out past 10¹² hung usvg's layout, and a
  picture a billion times wider than tall panicked it — which nothing
  here can reach, and in the browser a panic cannot be caught. So the
  file is asked first (`past_reason`): a number past a billion in any
  attribute or style sheet, or a picture more than a million times
  wider than tall, is refused with a sentence saying which, leaving out
  what is not a number (ids, classes, links, colours in hex, a
  picture's data, other programs' attributes). A compressed SVG is
  unpacked here rather than by the reader, so it is asked the same, and
  refused rather than unpacked past 256 MB. Then 3,600 more, with
  millions and millionths among the extremes so that what passes the
  door is exercised too: none fatal
  (`a_number_past_any_drawing_is_refused_before_the_reader_sees_it`,
  `a_drawing_scaled_past_reason_comes_in_without_stopping_anything`).
- **usvg 0.48, taken (October 2026).** The call below was made: the
  reader is 0.48.1 (resvg, a test dependency, with it). Nothing in the
  engine changed to take it — `Place::text` had already made the
  importer right under both — and every SVG test held, the 400-page
  round trip and the hostile-number sweep among them. The WebAssembly
  measured 2.37 MB compressed before and 2.77 MB after (+403 KB, as
  first measured); `past_reason` stays at the door, since a reader's
  fixes are no reason to hand it numbers past any drawing. The record of
  the first trial, as it was written:
- **usvg 0.48, tried and measured, not taken (yet).** It mends several
  of the reader's own crashes at the source (bounding boxes past the
  integer range, non-finite values, `feComposite`), fixes text advances
  and a transform applied twice, and moves text onto `skrifa` and
  `harfrust`. Bumped, everything compiled unchanged and two tests failed,
  both the 400-page round trip and both one real thing: the newer reader
  hands a text's glyph outlines over already placed while its underlines
  still come in the text's own space, and the importer placed every
  outline itself, so a turned caption was turned twice. The importer now
  reads every outline inside a text in the text's own space whatever the
  reader says, which is right under both versions (`Place::text`), and is
  in. The bump itself is not: the engine's WebAssembly grows from 2.33 MB
  to 2.73 MB compressed (5.96 to 7.19 MB raw), four hundred kilobytes on
  every load of the browser app, for a reader whose known failures are
  already refused at the door (`past_reason`). That is a call about the
  product's weight rather than its correctness, and is left to make —
  with nothing else in the way of making it.
- **Colours used lately:** under the document's palette, a row of the
  last eight colours used — given to a layer from the palette or the
  row itself, or chosen in the system's picker (its `change`, not every
  `input` of a drag through it) — newest first, round so they do not
  read as palette entries. It is `prefs.recentColors`, kept across
  documents because it is the person's habit rather than the file's; a
  click gives one by value, so nothing follows it the way a named
  palette colour is followed (smoke block 9j).
- **WebP, out and in.** The Export window offers a lossless WebP
  beside the PNG: the same picture, transparency and every pixel (the
  engine's test decodes both; the smoke suite compares the two files
  the browser decodes, byte for byte), usually far smaller — the smoke
  page is 7 kB as a PNG and 0.4 kB as a WebP. It goes everywhere a PNG
  does: the page, picked layers' box, a region in its own shape, a
  slice per kept region, any scale or the @1x–@3x set
  (`Raster::{Png, Webp}` in `core/codecs` is the last step of every one
  of those paths; `export_webp_at`, `selection_webp`,
  `kept_region_webp` on the wire). Only the *encoder* is in the engine
  — +10 kB compressed. The decoder would be another 58 kB for something
  every webview does, so placing a WebP — and a GIF or a BMP, which the
  engine never read either though a drop offered them — goes through
  the browser, which hands the engine a PNG of what it draws
  (`app/src/picture.ts`, decided by the bytes rather than the name).
  A lossy WebP is not offered: the encoder has only the lossless half,
  and the JPEG is the lossy picture. The desktop shell opens `.webp`
  files as it opens PNGs (smoke 8f2 and 9bh).
- **Posterize and threshold.** Two adjustment layers the classic set
  had and this did not. A posterize holds each channel to a few steps
  (`levels`, read whole and held to 2..=255), evenly spaced *as shown*
  — even in light they would crowd every band but one into the
  shadows; a threshold makes each pixel white or black by the
  brightness it shows at, so pure blue, bright to a sensor, is black.
  Both renderers draw them (GPU kinds 20 and 21; the step counts and
  the level are worked out once on the CPU side and handed over, and
  the shader rounds a half up as Rust does, since WGSL's `round` goes to
  even), and the random pages now hold them, sharing the twelfth slot
  of the adjustment choice four ways so the other eleven draw as they
  did. The mirror test now holds the UI's `Adjustment` and `Filter`
  unions to this crate's kinds both ways — serde names every variant
  when asked for one there is not — which it did not before: a kind
  left out of `engine.ts` passed it (`a_picture_is_held_to_steps_or_to_two`,
  the GPU's adjustment list, smoke 9o2).
- **Select the same.** Select › Select the same fill colour, stroke
  colour, or kind of layer picks every layer that shares it with the
  picked one — Illustrator's Select › Same — leaving out what a click
  could not reach (hidden, locked, or in a group that is). A colour is
  the same when it looks the same, to half an eight-bit step, so a
  palette colour and the same colour typed in are one; a gradient fill
  is no one colour (`Session::alike`). It came with the change that
  makes it worth having: a colour given from the palette or the recent
  row now goes to *every* picked shape and text, as one step in the
  history, where it went to the first picked one alone (smoke 9o3).
- **Outline the stroke.** Layer › Outline the stroke turns a shape's
  line into a shape of its own, filled in the line's colour, so it can
  take a gradient, be combined or be reshaped (Illustrator's Outline
  Stroke). The page does not change: a shape with no fill becomes the
  outline; one with a fill becomes a group of the fill and the outline,
  the group carrying blend, mask, effects and place and each of the two
  the fade, since a layer fades its fill and its line each as it lays
  them (`Session::outline_stroke`). The region is the renderer's own:
  a plain rect's or ellipse's band is two exact rims, and anything else
  — caps, joins, dashes, markers, swelling widths — is its stroke pieces
  sampled eight to the pixel, traced, relaxed (no corner moved more than
  half a cell) and straightened, within a quarter of a pixel of what is
  painted (`chitrakar_render::stroke_outline`). It went through the
  shape booleans first and they could not trace a third of 300 random
  strokes: a dash folding back over itself lays pieces along the same
  lines, which an edge-classifying boolean cannot be sure of. Sampling
  cannot fail to close; the same 300 now outline with nothing wrong
  (`a_stroke_nobody_chose_outlines_to_what_it_paints` keeps 80 of them,
  smoke 9o4). Before building this, a grow/shrink for regions was
  written and thrown away — `Session::grow_selection` already existed,
  exact and traced; the toolbar's Grow box is its door. Look for the
  engine function before the menu row.
- **Offset path.** Layer › Offset path… puts a copy of the picked
  shape just above it with its outline moved out by a distance, or in
  by a negative one — a sticker's border, a keyline round a logo
  (Illustrator's Offset Path). The shape is left as it was and the copy
  wears its look. A distance, not a scaling: a corner moved out grows
  round, one moved in stays sharp (`Session::offset_path`,
  `chitrakar_render::offset_outline`). A rect's is exact, an ellipse's
  moves both radii as its stroke does, and a path's is the shape plus
  or minus a band of that half-width along its outline — holes and the
  edge an open path is filled across included — laid on the same grid
  that outlines a stroke (`SampleGrid`, shared with `stroke_outline`),
  judged against distance to the outline on 40 random paths
  (`an_offset_outline_is_the_shape_moved_by_a_distance`, smoke 9o5).
- **Simplify path.** Layer › Simplify path… says a path's outline
  again in as few anchors as stay within a distance of it — straight
  where it runs straight, cubic curves with handles where it bends,
  corners kept as corners — which is the step after outlining a
  stroke, offsetting or combining: those leave hundreds of short
  steps that draw well and cannot be reshaped by hand (an ellipse
  offset by six: 103 anchors, then 6). Corners are found first (a
  turn sharper than about fifty degrees at one point) and each
  stretch between them fitted by Schneider's method — least squares
  for the handle lengths, Newton for the parameters, split at the
  worst point — with a stretch along its own chord kept straight, so
  a polygon stays a polygon (`chitrakar_render::fit`,
  `Session::simplify_path`). Holes stay straight-sided, as a path's
  holes are; a line that swells along its anchors is refused. Finding
  corners up front was pinned only after breaking it passed every
  test: splitting at the worst point finds them anyway, but starts
  the curve off each along the wrong tangent — a D in four anchors
  becomes seven (smoke 9o6).
- **Turn the text into shapes.** Layer › Turn the text into shapes
  replaces a text layer with a group of shapes, one per letter, named
  for it and in its colour, and a band for each underline and
  strike-through — what a print shop asks for, and what lets a letter
  be reshaped, filled with a gradient or combined
  (`Session::text_to_outlines`, `chitrakar_render::text_outlines`). The
  outlines are the face's own curves placed the way the PDF exporter
  places them (size, lean, the line a block is set along), flattened
  under a twentieth of a pixel; contours a font overlaps under its
  nonzero rule are said again for a path's even-odd one; a bold no face
  supplies is the outline swept along the baseline as the page draws
  it, copies an eighth of a pixel apart filled by the nonzero rule on
  the sampling grid. The group is the layer as it was (place, fade,
  blend, mask, effects). Rendered before and after on faded two-colour
  text with a bold leaning underlined word and on a line set along a
  curve: a mean difference of 0.0007 and no pixel far off; leaving the
  sweep or the lean out each fails it (smoke 9o7).
- **Reduce noise.** A median filter layer (`Filter::Median`, Filter ›
  Reduce noise): every channel the middle value within a radius, so a
  speck is outvoted and goes where a blur would spread it, and an edge
  between two flat colours stays exactly where it was. Huang's sliding
  window (`chitrakar_render::median`): a histogram per channel moved a
  column at a time with a running median, so a radius doubled costs
  twice rather than four times — the reason it was set aside once, a
  window scaled with the zoom being a square of the zoom otherwise.
  Binned in 256 steps of the display encoding by a search among the
  edges between steps rather than by encoding each value, which is
  what made it a third of a second for three megapixels at radius 1.
  Equal to sorting every window on random pictures; its reach is
  guarded by the region-render audit (breaking it fails there), its
  zero radius by the neutral-settings audit; the GPU draws it ✅ up to a
  radius of eight device pixels (below), and the random pages hold it one filter page in four of
  the last slot (smoke 9o8). Putting it there reshuffled the random
  pages, and the hostile-number audit found an older defect on a new
  one: every filter's reach stops at the most there is, but the sum of
  them was a plain `sum`, so two filters that both reached that far
  wrapped round to almost nothing — a region render padded by a few
  pixels where it needed all of them, or a panic in a debug build. It
  saturates now (`two_filters_past_any_reach_reach_as_far_as_there_is`).
- **Trim to the artwork.** Page › Trim to the artwork takes the page in
  to what is drawn on it, Photoshop's Trim choosing between its two
  usual answers by the page's own top-left corner: where that is clear,
  the clear pixels are the edge (a logo on nothing comes out the size of
  the logo); where it is painted, pixels of its colour are (a drawing on
  a white ground comes out the size of the drawing). Read off the page
  as drawn, to the eight-bit step, so a shadow is artwork as far as it
  reaches; the crop is the same `ResizeCanvas` the region crop uses,
  one step in the history (`Session::trim_page`, smoke 9o9 — whose first
  draft drew a shape the page's own proportions and so proved nothing).
- **Merge into a picture.** Layer › Merge into a picture draws the
  picked layers into one picture layer that takes their place —
  Photoshop's Merge Layers, and with one layer its Rasterize, so a shape
  or a text becomes pixels a brush, a clone or a heal can work on
  (`Session::merge_layers`, `chitrakar_render::draw_alone`). The layers
  are gathered into a group of their own in a scratch copy of the page,
  so they clip to and blend with one another as they did and an
  adjustment among them reaches only them — what merging an adjustment
  means, and the one way the page can change: the ground under them is
  no longer adjusted (tested both ways). Drawn at a pixel a page pixel
  on whole pixels, so the picture's pixels are the drawn ones rather
  than resampled; the box is what the members reach with their effects,
  widened by how far a filter among them spreads (leaving that out cuts
  a merged blur at the shape's edge, which the test catches). A hidden
  picked layer is left as it was (smoke 9o10).
- **GPU on screen (desktop shell, opt-in).** Preferences › General ›
  "Draw the page with the graphics card", offered only in the desktop
  app, has the shell draw the page's frames on the card. The route is
  (b) from "Next up" item 1, as a readback, so nothing about how the
  page is handed pixels changes. The engine in the webview stays the
  one the document lives in. The shell keeps a *mirror*: a new crate,
  `core/present` (`Presenter`), reached through five commands in
  `shells/tauri/src-tauri/src/gpu.rs`, and fed by `app/src/gpuFrames.ts`.
  - **How the mirror is fed.** When the document's JSON has changed it
    is sent over (`gpu_sync`). The mirror answers with the pictures and
    faces it lacks, and those go across once each as raw bytes:
    `gpu_picture` checks a picture's bytes against its content id before
    believing them, and `gpu_font` takes a face. The frame comes back
    as raw sRGB bytes for the engine's own viewport (`gpu_frame`).
  - **When the page draws instead.** The page draws the frame itself
    whenever the card would not draw it exactly as the processor does:
    - something the backend declines (`can_render_view`);
    - a CMYK page, a soft proof or a display profile
      (`frame_is_plain_srgb`);
    - anything still outstanding;
    - any error at all.
    So turning it on can only change how fast the page is drawn.
  - **A real bug found on the way.** At the identity view on a surface
    larger than the page, the GPU did not clip the artwork to the
    page's edge. The page bound was keyed on "is there a view" rather
    than "does the page cover the surface". It is fixed in `gather()`,
    and the witness gains that case and a layer over the far edge (the
    old rule fails it).
  - **Tests.**
    - Presenter tests run on llvmpipe: frames at 1× and at 2.5× panned
      match `engine_frame` byte for byte to within rounding; wrong
      picture bytes are refused; an edit resyncs without asking again;
      what is declined is declined.
    - Smoke 9ca's stand-in shell gains a stand-in card. Checked: the
      card's frame is shown, the picture is sent once with its size,
      and an edit resyncs without sending it again. When the card
      declines, the canvas is exactly the page's own frame — drawing
      only the dirty region there leaves the card's frame showing, and
      the check catches it. Turned off, the card is not asked.
  - **Why it is off by default.** It stays off until it has been timed
    in a real shell on a real card. This container has neither, and
    the IPC copy of a frame is the cost nobody has measured.
- **Channel mixer.** Layer › Add adjustment › Channel mixer makes each
  channel again from all three: the red that comes out is so much of the
  red, the green and the blue that went in, plus a constant
  (`Adjustment::ChannelMixer`, `chitrakar_render::channel_mix`).
  - **In light**, like the black-and-white mix it generalizes, and *not*
    normalized: a row that adds to more than a hundred brightens its
    channel on purpose. The panel says what the row adds to, in the
    note colour when that is not a hundred. Weights are held to ±200%
    and constants to ±100%, as Photoshop holds them.
  - **Monochrome** is one grey channel mixed by the red row, constant
    included. It is folded into the matrix (every row the red one), so
    both renderers mix by the same nine numbers.
  - **The GPU** draws it as kind 22, with nine numbers across the quad's
    ten free slots. A mixer with a constant needs twelve; it was handed
    back rather than rounded through an f16 table, but every surface the
    GPU draws on is `Rgba16Float`, so a constant on a half-float texel is
    rounded no further than the pixel it lands on. It is drawn now ✅ as
    kind 25, the constant on a one-texel table, held to the CPU with a
    constant that lifts one channel and lowers another, monochrome and
    not (dropping the constant, or only its negative part, fails it). A
    two-texel split, half-float plus remainder, was tried and could not
    be told apart, which is how the surface's own precision came out.
  - **Tests.** The random pages hold it near the identity, with every
    weight moved, sometimes monochrome and sometimes with a constant.
    Smoke 9o11 covers: red made from the blue (a red patch goes black, a
    blue one magenta); each output channel keeps its own row and a dot
    marks the changed one; 150% says so; monochrome hides the channel
    picker; undo.
- **Sides and skew.** The selection box has handles in the middle of
  each side, not only at its corners. A side stretches the layer along
  one axis, and only that one, even when dragged off its line; Alt holds
  the middle, as at a corner.
  - **Skew.** With Ctrl (⌘) held, a side slides along itself instead,
    leaning the layer, and the far side stays put. That is Photoshop's
    free-transform skew, and it never snaps, since a lean has no line to
    catch. The drag is measured from where it was picked up, so a side
    taken anywhere along its grip does not jump.
  - **Several layers.** A box round several picked layers stretches and
    leans the lot, by the same document-space change put back into each
    layer's own space.
  - **Not on frames.** A frame's sides resize it, as its corners do; a
    frame is not leaned.
  - **The Skew field** in the geometry row says the lean as a number,
    positive with the top to the right, as italic leans. A transform is
    read as a turn, then a lean, then a scale. The lean is what is left
    of the second axis once the turn is taken out, so typing an angle
    leaves the lean alone and typing a lean leaves the angle alone.
    Typing either keeps the middle of the layer's own box where it was,
    and the lean is held to ±85°.
  - **Smaller boxes.** A box too small to hold side handles apart from
    its corners, under 36 screen pixels along a side, shows none.
  - **Smoke 9o12** covers: a side stretches one axis (the old rule, where
    a side kept the proportions, fails it); north from the bottom; a
    Ctrl-drag leans 26.6° with the bottom held and the picture leaning
    with it; one undo; a typed lean about the middle; a turn keeping the
    lean; two picked layers stretched by the one box.
- **A picture's corners: distort and perspective.** Ctrl (⌘) on a
  picture's corner handle moves that corner of the picture on its own,
  and the picture is laid onto the quad its four corners make. That is
  Photoshop's Distort: a sign squared up, a screen put onto a photograph.
  - **How it is stored.** `RasterRef::corners`, additive and absent from
    every older file, holds where the four corners land in the layer's
    own space. The picture is carried there by the one projective map
    that takes its rectangle onto them (`chitrakar_doc::Projective`), so
    straight lines in it stay straight. A quad that is not convex has no
    such map and is drawn as the rectangle; a drag that would fold it
    stops at the last quad that lay flat (`Session::corners_command`
    refuses it). Once a corner has moved, each corner wears a round pin
    to drag. Layer › Put the picture's corners back is one step.
  - **The CPU** (`draw_raster_warped`) uses the same texels and bilinear
    tap as an upright picture. The footprint is read at each pixel from
    the map's own slope, since a perspective shrinks the far end more
    than the near. Its taps sit a texel apart, not two: at 4.9 texels a
    pixel, three taps two apart left a fine checker a third uncancelled
    (the test squeezes 64 texels into 13 pixels so no pixel lands on a
    texel edge by luck, and catches it). Edges are covered by exact area,
    and the box, the dirty region and hit-testing are the quad's.
  - **The GPU** hands a warped picture back to the CPU.
  - **Export.** An SVG gets the pixels it covers (`warped_pixels`),
    placed where the quad is; a PDF draws it as the page draws it.
  - **Two audit finds on the way, both fixed.**
    - `opaque_page` made every colour opaque but not a picture's own
      see-through texels. Two by two they never made a flat stretch for
      an audit to read; laid across a quad they do, and a reader mixing
      in sRGB parts from the engine there.
    - A copy of a blended layer that goes to a PDF as pixels landed
      Normal, its original's blend lost; it lands by that blend now
      (`landing_blend`, tested against ghostscript).
  - **The random pages** give a fifth of their pictures corners, drawn
    from a stream forked off the page's own so every older page is the
    page it was. A first version drew from the shared stream, reshuffled
    every page after it, and turned up older exporter differences that
    had nothing to do with corners.
  - **Tests.** Smoke 9o13: a Ctrl-drag lays the picture on a trapezoid;
    pins appear; a pin moves a corner alone; a fold is refused; undo;
    the menu row and its undo.
- **Photo filter.** Layer › Add adjustment › Photo filter is a coloured
  glass in front of the lens (`Adjustment::PhotoFilter`): every colour
  multiplied by the glass and mixed back in by a density, with "Keep
  brightness" scaling each pixel back to the brightness it had, in light.
  It opens as Photoshop's warming filter at 25%; Warming, Cooling, Sepia
  and Green are a press away, and any other glass is the colour well's.
  - **Shown values.** The multiply is on the values a device shows,
    where a density is judged. The first version multiplied in light,
    and a quarter of an amber glass over white came out (255, 251, 243)
    once its brightness was put back: the tint landed where light is
    spread thinnest on the eye. It is (255, 251, 213) now. What is above
    white goes through the glass in light.
  - **The glass is a colour like any other.** It can stand for a palette
    entry and follows it (the walk's own test catches it left out), it
    goes through a CMYK document's press profile (resolved in `prepare`,
    as a gradient map's stops are), and its alpha thins it as density
    does (`photo_filter_glass`, which both renderers lay by).
  - **The GPU** draws it as kind 23.
  - **Tests.** The random pages make half their channel mixers photo
    filters, from a forked stream, so no other page moves. Smoke 9o14
    covers it.
- **A window shows the page's own pixels, effects and all.** Zoomed in,
  a shadow, an outline or a blur at the edge of the window came and went
  as the window was panned across it. The window was drawn only to its
  own edge, and a layer's surface for its effects stops at the
  destination's edge, so whatever lay past the window was cut out of
  every effect inside it (the test finds the outline 0.39 covered on the
  page and nothing in the window).
  - **The fix.** `render_cached` now draws a margin past the window: the
    filters' reach, as before, and twice the furthest any visible effect
    is drawn from what casts it (`chitrakar_render::effects_reach`),
    never more than 1024 px. It goes into a scratch surface moved over by
    the margin, and only the window is kept. The page's own edge still
    stops what is drawn. Twice the reach, because a cut at the reach
    itself meets the last of a blur's tail.
  - **Tested** (`a_window_shows_the_effects_of_what_is_past_its_edge`):
    at 1× and 2×, a window panned so a bar with a shadow and an outline
    runs off its edge equals the page there. Without the margin it does
    not.
  - **Tried first, and backed out: casting from past the page's own
    edge.** That would let a shape running off the page throw its shadow
    back across it, as an SVG reader draws it; the SVG coverage audit
    found that on pages a fixture change had reshuffled. Done for a
    single layer's surface it works (a shape alone matches the same
    scene on a larger page, cut back). But a group, a copy or a masked
    layer is drawn through more than one surface, and every one would
    have to reach past the edge together. Wrapping a layer in a group
    and unwrapping it then moved the page by up to 0.009, and the wrap
    audit caught it.
  - **Still open.**
    - (A plain shadowed shape in a group, wrapped and unwrapped, also
      differed by 0.009 under the old rule. That turned out not to be
      the edge at all but a heal's average; see the next entry. Fixed.)
    - Making every surface reach past it together, and an SVG export
      that clips an effected layer to the page so readers agree in the
      meantime, is the follow-up. A second attempt, made after the heal
      fix below, drew every region on one surface reaching past it by
      twice the effects' reach and cut back (`region_at`, and `render`
      through it). Every nested surface then shares the same room.
      - It passes a page-against-larger-page test for a shape alone, in
        an effected group, as a copy of one parked off the page, and
        under a mask that hides nothing.
      - It fails the wrap audit (shadow-only shape in `fixture::
        everything()`'s group, copies hidden) by 0.037, at a blob inside
        the page along the shape's lower edge. With no room it agrees to
        1e-7, and the same with any room size (40 or 60).
      - Translating the whole page by whole pixels changes nothing. What
        differs is the windows. Wrapped, the shape draws into its group's
        surface with clip `{0,4,89,98}` and extent from −8.9. Unwrapped,
        the clip is `{0,0,107,96}` and the extent from −1.6. The group's
        own surface origin moves a pixel (−19 against −18 on the page).
      - Start there: which of those cuts reaches the shape's shadow.
- **A heal's average no longer turns on a pixel that is barely there.**
  The 0.009 a shadowed shape moved the page by, wrapped in a turned
  group and let out again, was a healing stroke lifting from the
  shadow's last tail, not an edge.
  - **The cause.** A heal takes the average of what it lifts and of what
    it lands on. A pixel counted in both averages whole, or not at all,
    by whether either side had any alpha. A tail pixel a millionth there
    in one composition and spent in the other took a solid pixel from
    under the stroke into the average, or left it out.
  - **The fix.** Each side is now weighed by the other's alpha, so a
    side with next to no alpha is next to no part of either average.
    The same change is in both renderers: the CPU's sums, and the GPU's
    `fs_heal_terms` and `fs_heal_terms_aside`.
  - **Tests.**
    - `a_heal_does_not_turn_on_whether_a_faint_pixel_is_there`: a
      millionth of a layer over half the source moved the old heal from
      grey (179, 179, 179) to (132, 135, 206).
    - `a_shadowed_shape_wrapped_and_let_out_leaves_a_heal_where_it_was`:
      the bisected case, at the 0.009295 it found.
    - The GPU heal audit gains a faint lift. Its old scenes were opaque,
      so the shader's weighting went unasked, and without it the GPU is
      0.377 from the CPU.
  - **How it was found.** By hiding the other layers of the fixture one
    at a time: the group alone agreed, and the difference appeared with
    the clone layer whose heal lifts from the shadow.
- **Layer comps.** States of the page kept by name, Photoshop's layer
  comps: which layers are showing, where, and how strongly. Three
  versions of a poster, or a before and an after, live in one file a
  click apart (`Document::comps`, `Command::SetComps`, `LayerComp`;
  additive, so an older file reads with none).
  - **What a comp holds.** Where each layer was, not the layers, so
    nothing is copied and an edit to a layer's content shows in every
    comp. A layer added since is left alone; one taken away is passed
    over.
  - **Putting one back** is one step (`Session::apply_comp`, a `Batch`
    of only what differs). Where nothing differs it is no step at all.
    `comp_showing` says which comp the page is now, and the chip for it
    is marked.
  - **A page turned, mirrored, rescaled or resized after keeping a
    comp** carries the comp's top-level transforms with it (`map_page`).
    Otherwise putting the comp back would turn every layer back and
    leave the page turned (tested, and caught with the carry taken
    out).
  - **The panel.** A row of chips under the Layers header: a press
    shows a comp, shift-click keeps the page as it is under that name,
    double-click renames (a name already used is refused), ctrl-click
    forgets, and "+" keeps a new one.
  - **Tests.** `a_comp_puts_the_page_back_as_it_was_kept`, and smoke
    9o15: two arrangements, each back in a press, the mark following,
    one undo, ctrl-click forgetting. Without visibility in the comp,
    the hidden square stays hidden and the block catches it.
- **Radial blur.** Layer › Add adjustment › Radial Blur is a smear about
  a point, Photoshop's in its two kinds (`Filter::RadialBlur`).
  - **Zoom** streaks outward, as a lens zoomed during the exposure does:
    `amount` is the share of each pixel's distance from the centre it is
    smeared over.
  - **Spin** sweeps round, as a turning wheel blurs: `amount` is the
    angle in degrees.
  - **The centre** is a share of the page's width and height, so it
    stays put when the page is resized. Smears are centred on the pixel,
    sampled between pixels (every line runs its own way), and capped at
    129 samples. The panel has Zoom/Spin, the amount, and the centre.
  - **Its reach** (`filter_reach`) is half the smear at the page's
    corners. The random pages hold it, from a forked stream, but too
    gently for any audit to notice a wrong reach. So
    `a_dot_moved_under_a_radial_blur_repaints_its_whole_streak` asks it
    directly: a dot moved under a strong zoom, then the repaint against
    the page drawn whole. With the reach taken out it fails. It first
    passed with the reach out because the dot was the ground's own
    colour and invisible, which the sabotage caught.
  - **The GPU** draws it ✅, as the motion blur's pass with the line
    worked out per pixel (`fs_smear` → `radial` in the shader): the
    centre on the surface, how far the line reaches, and the view's
    linear part, since the line is drawn in the space the filter
    lives in. `a_radial_blur_smears_the_way_the_cpu_smears_it` holds it
    to the CPU at the page's size and under an uneven scale, with the
    layers upright and in a turned, squashed group. It fails with the
    linear part dropped, the centre read off the surface, zoom and spin
    swapped, nearest sampling instead of bilinear, a sample count one
    short, or a cap of 9. (A reversed spin cannot fail it: the samples
    are symmetric about the pixel.)
  - **Found while writing it:** under a *turned view* the CPU cuts the
    page's edge per pixel after drawing (`region_at`) and the GPU does
    not, so anything that spreads (any blur) leaks into the corners of
    the turned page's box there. The app's canvas never turns the view,
    so nothing on screen showed it, but `render_view` did not decline
    it. It declines it now ✅: a turned page whose edge crosses the
    surface goes back to the CPU, and one seen only from inside (no
    edge on the surface to cut) is still drawn
    (`a_turned_view_is_drawn_only_where_the_page_has_no_edge_to_cut`;
    unguarded, a blur under the whole turned page was 0.55 out in the
    box's corners).
  - **Tests.** `a_radial_blur_smears_out_from_its_centre_or_round_it`
    (catches zoom and spin swapped), and smoke 9o16.
- **Find layers ✅** — a field and a kind picker over the Layers panel.
  A word narrows the list to the layers whose names hold it, with the
  groups they sit in shown dimmed beside them (folded or not, so a
  match is never hidden inside a closed group); the kind picker —
  shapes, pictures, text, adjustments, groups and frames — narrows by
  what a layer is, and the two together by both. Typing there types,
  it picks no tool; Escape puts the whole list back, and an empty
  result says so. UI only: nothing about it is in the document.
  Smoke block 9o17.
- **Comps in the shared fixture, and a comp that follows a layer ✅.**
  `fixture::everything()` keeps a comp now ("quiet": the pair hidden,
  the loose shape faded and moved), so every audit is asked of a
  document that holds one. One audit stopped holding, and rightly: the
  file format's inventory grew by the twelve fields a kept comp cannot
  be missing. Everything else held, so the question became what *would*
  break a comp, and it was a layer changing parent. `reparent` and
  `ungroup_node` rewrite a layer's transform so it stays put on the page,
  but a comp kept before went on holding the old numbers, in the old
  parent's space: showing it afterwards put the layer somewhere else on
  the page, and a comp of a page nobody had changed stopped reading as
  showing. Both now carry the comps in the same batch (one entry, undone
  together): a reparent by the same `carry` the layer gets, an ungroup by
  the group's placement *as each comp kept it*, with a child hidden where
  that comp hid the group. `a_comp_follows_a_layer_into_another_group_and_out_of_one`;
  each of the three carries taken out fails it. Grouping adds an
  identity group, so it moves nothing and needs no carry.
- **Colour lookup ✅** (Adjust › Colour lookup…, or "+FX"): a `.cube`
  file's 3D table as an adjustment layer, the thing a grade made in
  Resolve, a film emulation or a camera's log conversion is handed over
  in (`Adjustment::ColorLookup { table, size, name }`).
  - **The file** (`chitrakar_codecs::cube`): 3D tables read red-fastest;
    1D tables spread over a 33-point grid; `DOMAIN_MIN/MAX` and Resolve's
    `LUT_*_INPUT_RANGE` resampled onto nought to one; grids past 65
    resampled down. Anything else is refused with a sentence saying
    what, which the app shows.
  - **The table** is a *resource*, like a picture's pixels: immutable,
    content-addressed, written into a `.chitra` as a PNG, so the
    container learnt nothing new. A resource is 8 bits a channel, so
    each entry is kept as 16: high bytes in the top half of the image,
    low in the bottom (`chitrakar_doc::lut`). Loading a table is not a
    history entry; the layer is.
  - **Both renderers** read it on the values a device shows, trilinearly,
    red then green then blue. A table that is missing or the wrong shape
    leaves the picture alone. The GPU lays the blue slices as a square
    of tiles (a row of 65 slices of 65 would be wider than a texture
    goes) and reads the eight entries by hand.
  - **Carried:** into a file, and on the clipboard into another
    document (`collect_resources` takes the table, or the paste is a
    layer that silently does nothing; the sabotage confirms it). The
    shared fixture holds one, gently warm at half strength, so every
    audit carries a table. Putting it there stopped two audits holding.
    The format's inventory wanted the table and the grid, rightly. The
    other was the file audit's own list of what a document refers to,
    walked by hand (pictures, picture masks) exactly as the clipboard
    walked it, and both were blind to a table the day it became a
    resource. So there is one walk now, `Node::each_resource`,
    exhaustive over every kind, adjustment and filter the way
    `each_color_mut` is, and both read it. With the table's arm taken
    out, the clipboard test and the file audit each fail.
  - **Tests.** Five on the parser, two on the resource, one CPU render
    (exact swap between grid points; half opacity; missing and misread
    tables), one GPU against the CPU at grids of 2, 17 and 65 (fails
    with r/g swapped in a tile, one row of slices, read in light,
    nearest instead of trilinear), one engine (file and clipboard), and
    smoke 9o18: a swap table through the +FX list, the panel naming it,
    "Load another" in place and undone, and a file that is not a table
    refused with nothing added.
- **Layer comps to files ✅**, and a JPEG of a region that is a JPEG.
  The export window's Area has "Each layer comp" beside "Each kept
  region": the page once for every comp, named for it, in PNG, WebP or
  JPEG, multiplied by the @1x/@2x/@3x set like slices are.
  `Session::comp_raster` / `comp_jpeg` show the comp on the document,
  draw, and put it straight back (`with_comp`), so neither the page nor
  history moves; drawing a copy instead would copy every picture's
  pixels once a comp. `a_comp_exports_as_it_shows_and_moves_nothing`
  exports the comp that is *not* showing last, since a version that left
  its comp on the page passed when the last one exported was the one
  already showing; with the restore taken out it fails now. Smoke 9o19.
  - **Found on the way:** JPEG with "Each kept region" wrote a *PNG's
    bytes* under a name ending `.jpg`, because the slice path only knew
    PNG and WebP. A JPEG of a region is now the box that holds it, on
    white, as a JPEG of what is picked already was
    (`Session::kept_region_jpeg`), and smoke 9by reads the files' first
    bytes (FF D8) and fails with the old path.
- **The GPU draws a median ✅** (radius up to eight device pixels; past
  that it goes back). A sliding histogram is not something a fragment
  can keep, but a vote among 256 steps is eight halvings: count how much
  of the window is at or below the middle of what is left, keep the half
  the median must be in, all four channels at once. It rides the smear
  pass (`fs_smear` → `median`), reading the backdrop copy with the
  page's edge carried on past it, as the CPU's snapshot does.
  `a_median_votes_the_way_the_cpu_votes`: a speckled picture with clear
  patches, at radii 1, 3 and 8, faded, and under a view that enlarges
  it (one that shrinks the picture is handed back for the picture's own
  sake). It fails with the upper middle for the lower, seven halvings,
  the radius one short, or steps counted in light. Holding a colour to
  its cover cannot be caught and need not be: colour is premultiplied,
  so each pixel's colour is at most its alpha and so is the k-th of
  each, which leaves the hold for rounding between steps.
- **The GPU draws a shrunk picture ✅**, which is a photograph looked at
  zoomed out: the most ordinary view there is, and until now one the
  desktop shell's GPU path handed back every time. The CPU averages a
  grid of bilinear taps (at most four a side) over what each pixel
  covers; the grid is worked out by one function both renderers call
  (`chitrakar_render::picture_taps`) and carried on the image quad, and
  `fs_image` takes the same taps. `a_shrunk_picture_is_averaged_the_way_the_cpu_averages_it`
  holds it at a half, three tenths, a sixth, shrunk along one axis,
  turned and faded, on stripes a texel wide where a single tap is a
  different picture: one tap fails it (except at exactly a half, where
  one bilinear tap *is* the two-by-two average), as do taps a texel
  apart instead of spread over the footprint.
  - **Found on the way:** the skirt every image and shape quad is grown
    by, so its edge pixels can be faded by the area really covered, was
    one figure for both axes (the grow over the larger scale). On a box
    scaled unevenly the squashed axis got less than a pixel of it, and
    the edge row came out a quarter dimmer than the CPU's: four MSAA
    samples, three of them on the quad. Each axis is grown now by what
    moves its edges `grow` device pixels square to themselves (the
    determinant over the other axis's length). The one-axis case fails
    with the old growth. That was reachable before this, by a picture
    enlarged across and squashed down.
  - **And a photograph wider than 2048 ✅.** The device asked for
    `downlevel_defaults`, whose textures stop at 2048 a side: a camera's
    picture (four to six thousand across) and a hi-DPI window past 2048
    device pixels both went back to the CPU whole. A renderer now asks
    its adapter for up to 8192 a side and a buffer of up to a gigabyte
    (the readback grows with the window), and answers for what it was
    given: `GpuRenderer::draws_view`, which the shell's mirror
    (`core/present`) asks now. The checks with no renderer in hand
    (`can_render`, `can_render_view`) still answer for the guarantee, so
    tests running side by side in one process cannot see each other's
    device. `a_photograph_wider_than_the_guarantee_is_drawn_where_there_is_room`:
    a 3000-wide picture fitted to the page, and a 2500-wide window onto
    it, drawn and held to the CPU; it asks the *device* what it was
    granted before deciding to skip, since the first version asked the
    renderer's own note of it and so skipped, rather than failed, when
    that note was the thing broken.
- **The GPU draws a frame between pixels ✅**, which is every frame at
  most zooms. The CPU rounds a frame's box to whole pixels (a frame's
  edge is a page edge, and crisp); the GPU handed such a frame back
  rather than round it. It rounds it the same way now and lays the
  ground on the rounded box, where an antialiased edge is a crisp one.
  Of the two thousand random pages, 913 are drawn now against 801.
  Letting those frames through found three things the GPU drew wrongly
  in *any* frame and had never been asked to draw, because no frame of
  that kind had reached it; each goes back now:
  - a frame with a blend of its own holding something that works on the
    page (a curves layer in a frame set to Exclusion left the page as it
    was: the GPU draws a blended frame on a surface, where an adjustment
    has nothing under it, and the CPU draws it in place as well);
  - a frame held to the layer under it (the frame arm returned before
    the cut every other layer takes, so the whole rectangle showed);
  - a filter reading a neighbourhood inside a frame (on the CPU it stops
    at the frame's edge; the GPU's passes read to the page's, so a
    pixelate's blocks along every side came out a different grid).
  `a_frame_between_pixels_is_drawn_and_what_it_cannot_hold_goes_back`
  holds a plain frame, one with a vignette and one blended over paint
  to the CPU at its own size and at an awkward zoom, and asks the three
  to go back; laying the ground on the unrounded box fails it. The
  random-page audit's note on the one page over its drift level was
  stale (1529 had come under it); it is seed 970 now, on HEAD as before.
  What is still declined, by the same tally: frames turned, faded or
  masked (about 280 pages), and 374 pages that need more than one layer
  taken away before the GPU will draw them.
- **Colorize ✅** on Hue/Saturation (a box under its sliders):
  Photoshop's one-colour print. Every pixel keeps the lightness it shows
  at (HSL, on the values a device shows) and takes the one hue the Hue
  slider names, at a saturation of `(saturation + 1) / 2`, so the
  slider's -1..=1 reads as 0..=1 of the tint, and ticking the box from
  where the layer opens starts at a quarter, as Photoshop's does.
  Lightness lifts in light afterwards, as it does unticked.
  `HueSaturation { colorize }` is `#[serde(default)]`, so a file from
  before opens with it off (a doc test asks). One function works the tint
  out for both renderers (`chitrakar_render::colorize_tint`, which also
  holds a non-number to red at a quarter); the GPU carries it on the
  quad's second row. The random pages colorize a third of their
  hue/saturation layers, from a forked stream so no page moved.
  Tests: `colorize_lays_one_hue_at_the_lightness_each_pixel_shows` (a
  grey and a blue of one lightness come out one tint; paper and ink stay
  put; the hue is the hue asked; the slider maps, and fails read
  straight), two colorized layers in the GPU's adjustment list (fails
  ignoring the flag, or reading luma for HSL lightness), smoke 9o20.
  - **Found on the way:** several tests added in this stretch had been
    put between the test before them and *that* test's doc comment, so
    a neighbour's comment sat on top of the new test's and the
    neighbour had none. Four were tangled (two engine tests, one
    renderer test, one document test) and each comment is back on its
    own test.
- **The GPU draws a faded or masked frame ✅** that holds only paint:
  composited as a whole, on a surface of its own with its fade and mask
  taken once on the way down, which is what the CPU does with one. One
  holding something that reads the page (the CPU draws it in place and
  mixes it back) still goes back, and so does a masked frame wearing an
  effect. Random pages drawn: 948 of 2000, from 913.
  `a_faded_or_masked_frame_is_composited_whole`: faded, masked, both, and
  inside a faded group, at its own size and an awkward zoom, with two
  overlapping shapes inside so a fade taken per shape would show. Taking
  faded frames off the surface fails it.
  - **Open, found on the way:** seed 1233 holds a masked, outlined frame
    inside an outlined frame, in a copy hanging off the page's bottom
    edge, and the GPU and the CPU part along that edge. Taking the mask
    off still differs, on the code from before this change, which drew
    that version. Moving the copy up onto the page leaves eleven pixels,
    along the one edge it still hangs over. It is the same question as
    off-page content casting effects onto the page (above), so a masked
    frame wearing an effect goes back until that is settled. The
    unmasked case is not declined and is still apart.
- **Live polygons and stars ✅** (`VectorShape::Polygon { rx, ry,
  points, inner }`). The Polygon and Star tools used to lay down a path,
  so a star's count was decided before it was drawn and its depth never.
  Now both make a live shape: the panel has **Point count** and **Star
  depth** sliders (one entry each), and *Convert to path* gives its
  corners as anchors when a hand is wanted on them. It sits in the box an
  ellipse of the same `rx`/`ry` fills, first corner straight up; a star's
  every other corner is pulled in to `inner` of the way out
  (`VectorShape::polygon_corners`, which holds a count to 3..=100, a
  depth to 0..=1 and a size that is no number to nothing).
  - **One meaning everywhere:** a polygon is the closed path through its
    corners (`VectorShape::as_path`). The renderer, the GPU, SVG and PDF
    each turn it into that path once, where a shape layer is drawn, and
    the per-pixel functions answer for it by the same path where one
    reaches them alone (a mask). `flatten_shape` returns the path, which
    takes gradients, bounds and text on a polygon with it; `offset_outline`
    goes the path's way (an outline effect), where the rim rings that
    serve a rectangle and an ellipse would have given it none.
  - **The random pages** make a third of their ellipses polygons, half of
    those stars, from a forked stream. Two audits moved:
    - The GPU's rough-page ratchet went from 29% to 32% (to 33%): an
      ellipse's edge is exact on the GPU, while a polygon is a path
      antialiased by four samples. The interiors held on every page.
    - Ghostscript's flat-pixel audit on page 289. The triangle left
      flat ground where the ellipse's edge had been, beside text on a
      curve, and Ghostscript sets small turned type a pixel apart from
      the engine (same glyph positions, `text::placed`, and the same
      matrix, checked term by term; different rasterizer). It held
      without the polygon too, so it is not the polygon's. The two
      Ghostscript audits now ask that a pixel be flat in *both*
      pictures. Swapping red and blue in every PDF fill still fails
      them on page 0, with 1031 pixels.
  - **Tests:** `a_polygon_has_its_corners_where_a_polygon_does` (count,
    top, on the ellipse, clockwise, a star's two radii, the clamps, the
    path); `a_polygon_draws_what_its_path_draws` (pixel-identical filled
    and stroked, tips filled and notches bare); smoke 9j9 (depth to nothing
    fills a notch, eight corners reach past the pentagon, one undo each).
- **Angular gradients ✅** (`Gradient::Conic { center, angle, stops }`).
  A ramp that goes round a point instead of along a line or out from
  one: the colour wheels, dials, and cone highlights that a linear or
  radial ramp can only fake. `chitrakar_doc::conic_turn` is the one
  reading of it. The turn starts at `angle` degrees, clockwise on the
  page (0 points right, 270 straight up), and wraps once. At the centre
  it is 0. It is never NaN, whatever the angle holds. There is no
  spread, because it never runs past its ends.
  - **Drawn** by the CPU through `GradientGeom::Conic`, and on the GPU
    as ramp kind 2 in `ramp_at` (geom `[cx, cy, angle, 0]`), for shapes
    and stencilled paths alike.
  - **PDF** has no angular shading, so a conic-filled layer goes as
    pixels (`is_live` says no). **SVG** has none either: it goes as a
    512² picture of the ramp (`gradient_rgba8`) inside a pattern in the
    shape's box units, so it is still clipped to the shape and still
    follows it.
  - **In the panel** it is *Angular gradient* in the fill type, with a
    **Gradient start** slider. On the canvas there is a centre knob and
    an angle knob, the angle knob at 0.3 out. Each drag is one undo.
  - **Fixture:** a third of the linear ramps on the random pages are
    angular now (their own RNG stream, so nothing else moved).
  - **Importer fix, found on the way:** a picture tiling a shape in its
    box's units came back as a unit square. usvg leaves box-unit pattern
    contents in a group whose transform their children are not told
    about. The `Image` arm now takes that group's transform, as paths
    already did. It is `a_picture_tiling_a_shape_in_its_boxs_units_comes_in`.
  - **Open:** seed 156 puts a conic-filled path inside a Color-blended,
    outlined frame. It comes back from SVG as a picture in a pattern,
    and the frame's outline effect differs along it. `portable()` turns
    conic ramps linear before the round-trip audits, as it does for
    other things SVG brings back as something else.
  - **Tests:** `a_conic_gradient_turns_clockwise_from_its_angle`;
    `an_angular_gradient_goes_as_a_picture_inside_its_shape` (swapping
    the picture's axes fails it); `an_angular_gradient_turns_the_way_the_cpu_turns_it`
    on the GPU (a rect and a turned path; turning the other way or from
    the wrong side of the start fails it); smoke 2b (the ends meet
    straight up, the slider moves the seam, the knob puts it back, one
    undo).
- **Spread, choke, and glows ✅** (`spread` on `Effect::DropShadow`
  and `Effect::InnerShadow`, `#[serde(default)]`). A shadow could only
  be soft, so the panel had no glow and no hard-edged shadow that still
  feathers. Spread multiplies the blurred coverage by
  `chitrakar_doc::spread_gain` (`1 / (1 - spread)`, held at twenty at
  `SPREAD_MOST` = 0.95, and one for anything that is not a number), then
  caps it at full. The shadow stays solid further out but reaches no
  further: three box passes have finite support, so the reach is still
  the blur's, and `Effect::reach` is unchanged. An inner shadow's spread
  is its choke.
  - **Where it is taken:** in the stamp, after the field is read at
    its offset (`Gain` in `core/render`), so a second renderer reading
    the field with its own bilinear tap can follow the same order.
  - **GPU:** `gained` in `fs_effect`, or in `fs_settle` when the layer
    carries a blend, with the gain and the tint's alpha riding the
    quad's `extra`.
  - **SVG:** an `feComponentTransfer` linear slope after the
    `feOffset`. A reader clamps it, which is the engine's cap. The
    importer reads it back as the spread
    (`Shaped::spread`), and an outline is still told apart because it
    is a hard step of slope ≥ 100 with an intercept.
  - **PDF:** effects go as pixels, so nothing changed there.
  - **In the panel:** a **Spread** slider on a drop shadow and a
    **Choke** on an inner shadow. *Add effect* also offers **Outer
    glow** and **Inner glow**, which are shadows cast nowhere, light
    and hardened, and the row calls a shadow at no offset by that name.
  - **Fixture:** half the random pages' shadows are hardened (their own
    stream). `everything()`'s inner shadow is at 0.5, and both fields
    show up in its file.
  - **Open, found on the way:** seed 305 has a copy of a Hard Light
    brush layer that wears an inner shadow, and a copy of that copy.
    Through SVG they come back as pictures that carry the blend
    themselves, so the shadow goes down in Hard Light, not Normal over
    them. That was 56 pixels apart before spread; hardened, the shadow
    put 35 flat ones past the colour audit. `portable()` takes effects
    off a copy whose chain reaches a blended layer until the importer
    brings such a copy back as a group of its own around the blended
    picture.
  - **Tests:** `a_shadow_from_before_spread_opens_soft` (doc);
    `a_spread_hardens_a_shadow_without_carrying_it_further` (gain exact
    pixel by pixel, nothing where there was nothing, a choke darker
    inside, nothing outside; switching the gain off fails it);
    `a_spread_shadow_is_hardened_the_way_the_cpu_hardens_it` on the GPU
    (a glow, a fractional offset, a choke, and both on a screened layer;
    taking the gain off the stamp or off the settle pass each fails it);
    `an_effect_exported_comes_back_as_itself` now runs soft and
    hardened (dropping the spread on import fails it); smoke 8x2 (a glow
    on both sides alike, spread solid further out and no further, one
    undo a step, an inner glow lighting the edge, choked further in).
- **Justified text ✅** (`TextAlign::Justify`). A wrapping block can be
  set flush to both edges of its width. Each line's slack is shared out
  after its spaces (`text::word_gap`). The last line of each paragraph,
  any line without a space, and any block with no width are set to the
  left, which is what a reader expects of justified type.
  - **One place:** every space on the line takes a share, an indent's
    too, because that is exactly SVG's `word-spacing`. `text::set_line`
    is now the one place a line is shaped and placed. The rasterizer,
    `placed` (so the PDF's glyph-by-glyph text) and the underline and
    strike bands all read it, so the letters, the bands under them and
    what an exporter writes are the same line. `stretches` takes the
    glyphs instead of shaping the line again.
  - **SVG:** each spread line says `word-spacing` on its pieces, with
    the start at the left edge (`SetLine::gap`). The importer brings
    text in as outlines, so the round trip needs nothing of its own.
  - **In the panel:** a fourth alignment button, *Justify text*, and
    the canvas's typing box justifies when the block wraps.
  - **Fixture:** half the random pages' wrapped blocks are justified
    (their own stream).
  - **Tests:** `a_justified_line_reaches_both_edges_and_a_paragraphs_last_does_not`
    checks each line's ink against the same block set Left and set
    Right. It also checks the block's size, an underline reaching the
    right edge, and no change without a width; zeroing the gap fails
    it, and so does leaving the band's width unspread.
    `a_justified_block_leaves_as_the_engine_sets_it` has resvg ending
    every line in the engine's column; dropping `word-spacing` or
    halving it fails it. Smoke 8v2: justified, the folded lines reach
    further right, the block keeps its size, and setting it Left puts
    them back.
  - **The reader audit's allowance went from six pixels a page to
    ten.** Page 578's block is now justified, so it is set to the left,
    and it is twelve-pixel type turned 13°. The engine draws it as a
    resampled raster and resvg as outlines, and nine pixels along the
    foot of the letters come out 0.55 against 0.05. That is an edge half
    a pixel apart, which the audit allows for, and set to the right the
    same block has four. The audit itself does not see a halved gap,
    because the fixture's strings give few lines with a space to spread,
    so `a_justified_block_leaves_as_the_engine_sets_it` is what holds
    justification in SVG.
- **Surface blur ✅** (`Filter::SurfaceBlur { radius, threshold }`). This
  is edge-preserving smoothing, for sensor noise and skin: grain in flat
  areas goes soft while the edges between them stay. The filters had a
  blur that smears edges and a median that takes specks out, but
  nothing for this.
  - **How it works:** a bilateral filter, taken one axis at a time
    (`core/render/src/bilateral.rs`). First along each row, then down
    each column of that result. A neighbour weighs by a Gaussian of half
    the radius times its likeness: one for the same shown colour,
    falling straight to nothing once any channel is `threshold` apart.
  - **Why the display encoding:** likeness is judged there, which is
    where a person sees two colours as near or far. In linear light,
    every pair of shadows would be alike.
  - **Why separable:** a line of reads a pixel a pass rather than a
    square. That keeps the browser's CPU engine usable at wide radii,
    up to `SURFACE_MOST` = 64 device pixels each way.
  - **GPU:** the pixelate's two-pass arrangement, with `fs_block` sent
    to `surface_pass` by a 2 or a 3 in `params.w`. It computes the same
    weights per tap from the same radius, and it is declined inside a
    frame like every neighbourhood filter.
  - **In the panel:** *Surface blur* in the adjustment picker, with
    **Smoothing radius** and **Edge threshold**.
  - **Fixture:** half the random pages' medians are surface blurs
    (their own stream).
  - **Tests:** `a_surface_blur_takes_the_grain_out_and_leaves_the_edge`
    (grain more than halved, both sides of the edge keep their tone
    where a Gaussian of the same reach smears them, and nothing asked of
    it does nothing; making likeness always one fails it).
    `a_surface_blur_smooths_the_way_the_cpu_smooths` on the GPU checks
    four radius, threshold and opacity settings and a view half as large
    again. A plain-blur weight, a missing column pass and the wrong
    sigma each fail it. Smoke 9o21: grain over two greys is smoothed,
    the edge holds, and three undos bring the grain back.
- **Trace a picture into shapes ✅** (`Session::trace_picture`, Layer ›
  *Trace into shapes…*). A logo scanned from paper or an icon from a
  screenshot comes back as flat vector shapes, in a group just above the
  picture. The picture stays, so the trace can be compared, hidden or
  thrown away. One step in the history. A picture on four corners is
  refused. The work is `core/render/src/trace.rs`:
  - **Colours:** counted at five bits a channel, then chosen the
    k-means++ way, made deterministic. The commonest colour comes first;
    each next one has the most pixels times squared distance from those
    already chosen. Then six rounds of moving each to the middle of its
    pixels. All in the display encoding.
  - **Why not median cut:** it was the first attempt, and it was wrong
    for drawings. It splits by population, so when one colour is most of
    the picture every split lands inside it, and a red disc and a blue
    frame on white came back as white and one purple.
  - **Specks:** a pixel that at most one of its four side neighbours
    shares a colour with (a speck, or a one-pixel bump on an edge) takes
    its neighbours' majority. Islands smaller than (2·tolerance)², and at
    least 3 px², are dropped.
  - **Stacked:** the commonest colour is at the bottom, and each colour
    covers its own pixels *and every pixel above it*. Shapes meeting
    edge to edge would each antialias half the seam and leave a hairline
    of whatever is behind.
  - **Outlines:** `trace_pixels` rings go through the middle of the step
    either side of each corner, so a staircase becomes the slope it
    stands for. A corner between two runs of three pixels or more is
    kept exactly. Then `fit::fit_curves` fits the outline, and holes are
    `straightened` (a path's subpaths are straight-sided).
  - **Size:** a picture longer than 1024 px is averaged down by a whole
    factor first, and the shapes are scaled back to the picture's units.
    The group takes the picture's transform.
  - **Tests:** `a_drawing_comes_back_as_its_shapes` (three colours,
    ground at the bottom with no holes, the disc a few curves on its
    circle, the frame exactly its four corners outside and in).
    Without the step midpoints the disc is 92 anchors; edge to edge,
    the stacking fails it; without the corner rule the corners move.
    `grain_does_not_come_out_as_specks`: without despeckle the frame
    grows bumps. `a_big_picture_is_traced_smaller_and_brought_back_to_size`.
    `a_traced_picture_draws_the_picture_again` (engine): placed at half
    size, traced, the picture hidden, under 3% of pixels differ, and
    undo removes it. Dropping the group's transform or the size scale
    fails it. Smoke 9o22: a canvas-drawn sign traced in three colours,
    picture hidden, shapes in place, undone.
- **The trace window ✅** (`TraceDialog`, `Session::trace_preview`).
  *Trace into shapes…* asked one question through the browser's prompt
  and drew blind. It is now a window with **Colours** (2 to 16) and
  **Smoothing** (0.5 to 4 picture pixels, the fit's tolerance), and the
  trace drawn as it is asked. `trace_preview` builds the same shapes
  (`Session::traced`, now shared with `trace_picture`) on a page of its
  own, at most 360 pixels long. It renders them there and hands back a
  PNG, so nothing is laid until *Trace* is pressed, and a preview is not
  a step in the history. The picture is redrawn a moment after the last
  change, so a dragged slider is not a trace a pixel.
  - **Tests:** `a_traced_picture_draws_the_picture_again` now asks for
    the preview first (the right size, the disc, bar and ground in their
    colours, history untouched); leaving the preview's scale off fails
    it. Smoke 9o22 drives the window: the preview at the picture's size
    with the disc in red, a new picture when the colour count changes,
    Escape laying nothing, *Trace* laying the group.
- **Paragraph spacing ✅** (`TextSpec::paragraph_spacing`,
  `#[serde(default)]`). The space after each paragraph: after every line
  the writer ended with a newline, never one the width wrapped, and
  never after the last. It is in ems of the size, as tracking is, held
  to 0..10 and nothing for what is not a number
  (`TextSpec::paragraph_gap`). The panel has a **Paragraph spacing**
  slider.
  - **One place:** the layout keeps each line's top (`Layout::tops`,
    `SetLine::top`). The rasterizer, `placed` (so the PDF), the
    underline and strike bands and the SVG exporter's baselines all read
    it, so the letters, their rules and every exporter move together.
    The block's height is the last top plus a step.
  - **The canvas typing box (fixed since):** it is a transparent
    textarea over the engine's letters, and a textarea sets every line
    in one size and stacks them evenly. Over a block with paragraph
    spacing, or (later) a run at a size of its own, the caret sat a
    line away from the letters it was among. Such a block is now typed
    as it is seen (`inline-text as-typed`): the box shows its own text
    in the block's colour over the panel's, and the engine sets the
    block when typing ends. A plain block keeps the invisible box.
    Smoke 8v1 holds both: a plain block's box is transparent, and a
    spaced one's shows its text.
  - **Tests:** `paragraph_spacing_opens_a_gap_after_each_paragraph_and_nowhere_else`
    (wrapped lines stay, the next paragraph and only its letters move
    by the gap, the block is that much taller and the ink with it,
    nonsense is no spacing); a gap after every line, or `placed` reading
    the old step, each fail it.
    `paragraph_spacing_leaves_as_the_engine_sets_it`: resvg starts the
    second paragraph in the engine's row, and dropping the top from the
    baseline fails it. `a_block_from_before_paragraph_spacing_opens_without_it`.
    Smoke 8v2: the two-paragraph block grows downwards only.
- **Blend between two shapes ✅** (`Session::blend_shapes`, Layer ›
  *Blend between the two…*). With two shapes picked in the same group,
  the space between them fills with *n* in-between shapes. Each is its
  share of the way from one outline to the other, and from one look to
  the other: fill, stroke colour and width, fade. They go in a group
  just above the lower of the two, stacked towards the upper, as one
  step. Unlike Illustrator's blend, which stays live, the in-betweens
  are ordinary paths. The two originals are untouched.
  - **Geometry** (`core/render/src/morph.rs`): both outlines are walked
    at equal steps of their own length into 128 points and matched,
    wound the same way and the second turned to start where it lines up
    best with the first. That is judged about each one's own middle, so
    where the two stand on the page does not twist the blend. Each
    in-between is fitted with curves (`fit_curves`, 0.25 px).
  - **Colour:** a colour blends component by component when both are
    said the same way (sRGB with sRGB, ink with ink). Otherwise it is
    whichever end is nearer, since two ways of saying a colour have no
    line between them. A compound shape blends by its outer outline.
  - **Tests:** `halfway_between_two_squares_is_the_square_between_them`
    (a square starting at another corner and wound the other way still
    gives the square between, every point on its edge); skipping the
    winding or the turn each fails it.
    `a_blend_lays_the_shapes_between_two` (engine): placed between the
    two even when asked from the upper; the middle is a square of twenty
    halfway along and exactly half purple; the steps grow in order; one
    undo. A colour that does not blend, or the order taken as asked,
    each fail it. Smoke 9o23: red to blue across a page, purple in the
    middle, redder near the red, gone in one undo.
- **A size per style run ✅** (`StyleRun::size`, `#[serde(default)]`).
  A block can hold a headline and the smaller line under it, or one word
  set large. The runs entry above said "a run's size is the block's:
  mixing sizes in one block would change where the lines sit, and is a
  separate thing". This is that thing.
  - **Shaping:** each piece was already shaped in its own run's
    styling, so advances, tracking and a thickened bold follow the size
    with no change.
  - **Lines:** `text::line_metrics` sizes each line by the tallest of
    what is set on it (its ascent and its step, per piece, in each
    piece's own face). Every letter on a line shares that line's
    baseline, and the line under a big run moves down to make room. An
    empty line is the size of what is set at its newline.
    `Layout::ascents` and `SetLine::ascent` carry the baseline.
  - **Drawing:** the rasterizer and text along a guide draw each glyph
    at its run's size (`Cut::px`). Underline and strike are as heavy,
    and as far from the baseline, as the size they rule.
  - **Export:** `placed` gives each run its own em, which is the size
    the PDF's `Tf` writes. SVG says `font-size` (`text::em_of`) on a
    run's piece and puts each line's baseline at its own ascent.
  - **In the panel:** the Size slider styles the selection when part of
    the text is picked out, as Bold and colour do, and reads the
    selection's size. `runs.ts` treats `size` as one more key.
  - **Found by the audits on the way:** a block's letter-spacing goes to
    SVG as a length at the block's size, so a run at its own size now
    says its own (seed 55). Text along a guide went into its `textPath`
    as one plain string, so every run on a curve came out in the block's
    own colour; that went unseen until a run was big enough to have flat
    pixels (opaque page 111). It is cut into tspans by run, as a line is.
  - **Fixture:** half the random pages' styled runs are set larger or
    smaller (their own stream).
  - **Tests:** `a_run_can_set_part_of_a_block_larger` (one baseline, the
    big size's; the line as tall as a block all that size; the run's em
    doubled and the rest's as it was; the next word pushed along; the
    big letters inked higher; the next line a big line down; nonsense
    sizes are the block's). Block-wide line metrics, block-sized glyphs
    in the rasterizer, and a block-wide em in `placed` each fail it.
    `a_run_at_its_own_size_leaves_as_the_engine_sets_it`: resvg's top,
    foot and right edge match the engine's; leaving `font-size` off, or
    the block's ascent for the line's, each fail it.
    `a_run_from_before_run_sizes_has_none`. Smoke 8v2: one word at twice
    the size makes the block taller and wider by that word, the slider
    reads the block's size over the other word, and Edit › Undo puts it
    back.
- **Repeat ✅** (`Session::repeat_nodes`, Layer › *Repeat…*). This is
  step and repeat: copies of the picked layers, each one step on from
  the last. A step is a move, a turn about a pivot some way from the
  middle of what is picked, and a scale about that middle. A row is a
  move; a ring is a turn about a point away from the middle (twelve at
  30° about a point below a mark is a clock face); a shell is a turn and
  a scale together.
  - **Stacking:** each set of copies goes above the last, so the repeat
    stacks the way it runs, and the whole repeat is one step. Each set
    is a copy of the originals through `emit_copy`. A copy of another
    picked layer points at the one in its own set.
  - **Masks and effects:** the step is said in the group's space, so the
    layers have to share a group. A copy's mask is laid in that same
    space and goes with it (`Mask::carried_through`). Its effects do not
    turn, because the light a shadow falls from is the page's.
  - **The window:** `RepeatDialog` asks copies, move, turn, scale and
    the turn's point. Enter repeats, Escape lays nothing.
  - **Tests:** `repeating_steps_each_copy_on_from_the_last`: a row
    stacked in order with each copy where its step puts it; a ring of
    three a quarter turn apart about a pivot below; halving sizes; one
    undo; a step that is not a number refused; and each copy's masked
    half drawn. Leaving the mask behind, or giving every copy the first
    step, each fail it. Smoke 9o24: Escape lays nothing, three copies
    sixty apart make a row of four with clear gaps, one undo.
- **Cut a path, join paths ✅** (`Session::split_path`, `join_paths`;
  Layer › *Cut the path at the anchor*, *Join the paths*). The anchor
  tool could add, take away and move anchors but never open or join a
  path.
  - **Cut:** a closed path opens at the anchor last pressed, running
    from it all the way round back to it, its ends on one spot to be
    pulled apart. An open path is cut in two at an anchor between its
    ends, the second half a layer of its own above the first and looking
    the same. The handles either side of the cut stay with their half,
    and so do a brushed line's widths.
  - **Join:** two open paths in one group join at the two ends nearest
    each other, either path turned round as that asks (its handles
    swapped in for out). Ends within half a pixel become one anchor,
    taking the first's way in and the second's way on. Ends apart are
    bridged by a straight segment. The second path is brought into the
    first's own space through both transforms. A path joined to itself
    closes, its coincident end dropped.
  - **The scissors ✅** (`Session::cut_path_at`): with the Node tool,
    alt-pressing a picked path's outline cuts it there, the pair to
    alt-pressing an anchor, which takes the anchor off. An anchor is put
    where the press lands and the path cut at it, as one step.
    `anchored` (what `insert_anchor` used to do, without applying it)
    and `split_commands` (what `split_path` does) are shared, so the two
    halves of the gesture are the same code as the two menu rows.
    `scissors_cut_a_path_where_it_is_pressed`: a press off the outline
    cuts nothing and leaves no step; a line pressed mid-way is two
    meeting at the press, drawn as it was; a ring opens at the press.
    Cutting at the anchor before the new one fails it. Smoke 9o26.
  - **Limits:** compound paths are refused, since their holes would
    belong to nothing. Each is one step. `path_rewritten` is the old
    `replace_path` body, kept for both and for the cut's second half.
  - **Tests:** `a_path_cut_and_joined_is_the_path_it_was` (a curved line
    cut at a middle anchor is two halves whose anchors are where the
    one's were and drawn the same away from the cut; joined, given the
    other way round, it is the four anchors again; an end refused; a
    line joined to itself closes, then opens at an anchor as five
    anchors ending where they start; two apart are bridged). Dropping
    the second half's handles, never merging met ends, skipping the
    carry into the first's space, or opening a ring without returning to
    its anchor each fail it. Smoke 9o25: a pen line pressed at its third
    anchor, cut, joined, undone.
- **A PDF for the press: bleed and crop marks ✅** (`PrintMarks`,
  `export_pdf_document_marked`, `export_pdf_frames_marked`,
  `Session::export_pdf_marked(frames, bleed_mm, crop_marks)`; *Bleed*
  in mm and *Crop marks* beside the PDF in the export window,
  `prefs.pdfBleed` / `prefs.pdfCropMarks`, which File › *Export PDF of
  the frames* follows too). A printer asks for artwork that runs past
  where the sheet is cut and for marks saying where. Before this a PDF
  stopped at its edge.
  - **The bleed:** the page is carried `bleed` past its trim on every
    side. Live vectors are already there, since the content stream was
    never cut to the page, and the stream is now cut to the bleed
    instead. What goes as pixels is rendered past the page by the same
    amount (`place_rendered` draws a surface the trim plus twice the
    bleed, shifted by it). The bleed is in millimetres at the session
    and turned into pixels by the document's dpi. It is kept to
    `MOST_BLEED_MM` (25), and nothing, a negative or NaN is no bleed.
  - **The marks:** with crop marks, eighteen points of slug past the
    bleed hold two 0.25 pt lines at each corner, on the lines of the
    trim, starting three points past the bleed so none prints on the
    kept sheet. They are in registration (`1 1 1 1 K` when separating
    for a press profile, so they show on every plate). The page says
    `/TrimBox` and `/BleedBox`, and its MediaBox is the trim, bleed and
    slug. Asked for neither, the PDF is byte-for-byte the one it was.
  - **Frames:** a frame clips its contents to its box, and its box is
    its page's trim, so at first a frame's page bled only white. For its
    page the frame now reaches out by the bleed: it grows by twice the
    bleed about its own corner, moves back by one bleed, and its layers
    move on by the same, so nothing moves on the page. Its colour, the
    artwork it holds and its pixel layers all run on past the trim, and
    nothing outside the frame does. A frame with a mask keeps its box,
    because the mask is in the frame's own space and says where it shows.
    `a_frames_page_bleeds_what_is_in_the_frame` (Ghostscript: a ground,
    a corner mark that must not move, a soft dab over the edge, a second
    frame's colour, a block beside it that stays out). Not growing, or
    not moving the layers back, each fail it.
  - **Setups:** a PDF kept as a setup keeps its press. The bleed and
    the marks are in its name (`PDF 3 mm marks`), come back when it is
    pressed, and decide whether it is the one in hand. A setup of
    another format leaves the PDF's settings alone, and a setup kept
    before these existed reads as no bleed and no marks. Smoke 9bw,
    which fails if pressing the setup does not bring its press back.
  - **Tests:** `a_page_for_the_press_bleeds_and_is_marked_where_to_cut`
    (Ghostscript: the sheet is trim, bleed and slug; the ground and a
    soft dab, which goes as pixels, run into the bleed and are cut where
    it ends; dark marks on the trim's lines, nothing across the bleed's
    corner; a bleed with no marks has no slug; asked for nothing, the
    page it was). Not shifting the pixels, not cutting, or not marking
    each fail it. It also found the frames export falling back to the
    unmarked page when there were no frames.
    `a_bleed_is_asked_for_in_millimetres_and_kept_to_a_bleed` (3 mm is
    8.504 pt at 300 dpi; capped; 0, −3 and NaN are none). Smoke 9e3: set
    in the window, exported with the boxes, remembered, and put back.
    Exporting the plain PDF from the window instead fails it.
- **Bevel ✅** (`Effect::Bevel { size, depth, lx, ly, altitude,
  highlight, shadow, opacity }`; *Add effect › Bevel*, with a swatch for
  each colour and an Angle slider). Photoshop's most-used layer style,
  and the one look in Affinity's and Canva's effect lists this did not
  have.
  - **The model:** the layer's silhouette is blurred by half the size and
    read as heights, a fully covered pixel standing `depth · size / 2`
    high. The slope is read off the eight pixels around each one with
    SVG's quarter Sobel. A distant light at `altitude`, from the direction
    `(lx, ly)`, then lights it. Where the slope faces the light more than
    the flat top does, it takes the highlight, by `(lit − flat)/(1 − flat)`.
    Where it faces away, it takes the shadow, by `(flat − lit)/flat`. The
    top is left alone, and the two halves never share a pixel. The result
    is painted over the layer and kept inside it (`bevel_field`,
    `bevel_light`, `bevel_shares`). The heights are taken over the layer's
    own opacity, so a faded layer's bevel is fainter but no flatter.
  - **Why a direction:** the light is a vector in the layer's parent
    space, carried through transforms the way a shadow's offset is. It
    was an angle first. Re-derived through `atan2`, it came back a hair
    off from even an identity carry, and four of the undo audits and
    *a page turned a quarter and back saves to the same bytes* caught it.
    The panel shows and sets it as an angle.
  - **It is SVG's lighting:** `feDiffuseLighting` under an
    `feDistantLight`, on the blurred SourceAlpha, with the surface scale
    and the azimuth written as the page has them, because resvg reads
    heights a device pixel at a time. One `feColorMatrix` per half turns
    the lit value into coverage, flooded and kept in by SourceAlpha.
    - The matrices give up half an 8-bit step. A reader rounds the flat
      top's lit value 0.5 up to 128/255, and without the dead zone the top
      wore a highlight of 0.4%. The sRGB curve near black turned that
      into thirteen levels.
    - The importer reads a highlight/shadow pair over one lighting back as
      the bevel it was (`lit_half`, `bevel`).
  - **GPU:** it draws a bevel as two stamps of the shadow machinery. The
    field is the silhouette in white, blurred by the same three boxes.
    `bevel_lit` in the effect shader lights each pixel from its eight
    neighbours, with the light in `params`, the height in `extra.x` and
    the half in `extra.z`. The two halves are disjoint, so two Normal
    passes lay what one would. A blended layer's bevel goes back to the
    CPU, since a blend takes over the stamp's pass.
  - **PDF:** a bevel is drawn as the picture beside the live layer that
    every effect is (`effect_pixels`).
  - **Found on the way:** the SVG importer read a blur in the filter's
    own units, but placed the layer, and its offsets, on the page. So a
    shadow from a group scaled by 1.25 came back a box radius too narrow,
    0.11 apart. `Page::blur` now answers in page units.
  - **Fixture:** the shared fixture's lettering is bevelled, over its
    inner shadow. A third of the random pages' effects get a bevel, drawn
    from the side stream that decides spreads, so every page drawn before
    is the page it was.
  - **Tests:**
    - `a_bevel_lights_the_edges_facing_the_light_and_shades_the_rest`:
      lit edges, shaded edges, a flat middle and nothing outside; it
      follows the light, fades with opacity, and draws nothing with no
      size or no direction.
    - `a_bevels_light_turns_with_the_space_it_is_written_in`: a quarter
      turn and back gives the same bits, and a mirror flips it.
    - `a_bevel_goes_to_svg_as_light`: resvg against the engine, in light
      rather than 8-bit sRGB, from four sides and in a turned, scaled
      group. The mean error is under 0.01 and the worst pixel under 0.12,
      where the terminator falls a pixel apart.
    - `a_bevel_exported_comes_back_as_itself`: the parameters come back;
      a turned group is measured against its own bare edges.
    - `a_bevel_lights_what_the_cpu_lights`: on the GPU, mean under 0.001,
      worst no more than the bare shapes' own edges.
    - Sabotages that each fail one of them: the azimuth flipped, the
      surface scale doubled, the slope's y flipped on the GPU, the opacity
      left out of the heights, the halves not told apart, the light turned
      the wrong way, the importer's old blur, and the highlight swatch
      editing the shadow.
    - Smoke 8x2c: a rect lit from the upper left, the light turned round,
      the angle read back, the highlight recoloured and undone, removed.
- **Round corners ✅** (`chitrakar_doc::round_corners`,
  `VectorShape::Polygon::corner`, `Session::round_corners(ids, radius,
  anchor)`; a polygon's *Corner radius* slider, and *Round by* with a
  *Round corners* button on a path). A rectangle could round its corners
  and nothing else could: not a polygon, a star, a pen drawing, or the
  outline a boolean leaves.
  - **The geometry:** a corner is an anchor with no handles whose sides
    either way are straight. It becomes two anchors where a circle of
    the radius touches those sides, joined by the cubic that draws the
    arc (handles `4/3·tan(sweep/4)` of the radius long). A corner takes
    no more than half of either side, so a radius too big gives the
    biggest arc that fits, and neighbouring arcs meet at a side's middle
    rather than crossing. These are not corners: the ends of an open
    path, an anchor its sides run straight through, and an anchor with a
    curve on either side.
  - **Live where the shape is a number:** a polygon or star keeps
    `corner` (serde default 0, so older files are sharp), and `as_path`
    rounds its corners. Every renderer and exporter already draws a
    polygon through `as_path` — the CPU, the GPU, SVG and PDF — so none
    of them changed. *Convert to path* takes the rounded outline, so a
    rounded star converted is still rounded. A rectangle's own `radius`
    answers the same command.
  - **A path is rewritten:** a path keeps no number for this, so its
    corners become anchors, as cut and join rewrite paths. That is one
    step, and a brushed line's width at a corner goes to both of its new
    anchors (`Rounded::from`). A smooth path or a compound one has no
    such corners and is passed over. With the anchor tool's last-pressed
    anchor on the one picked path, only that corner is rounded.
  - **Fixture:** half the random polygons and stars are rounded, drawn
    after everything else their stream gives a polygon, so every page
    before is the page it was.
  - **Found on the way:** `flatten_shape` returned a polygon's
    `as_path` unflattened, on the old promise that a polygon's path is
    straight. Rounded, the path is not straight. A rounded diamond was
    boxed by its anchors, short of the arcs that bulge past them. Drawn
    under Exclusion, on a surface cut to that box, it lost the sliver
    past it, and `a_blend_never_changes_what_a_page_covers` caught it on
    seed 656. `a_rounded_polygon_is_boxed_round_its_arcs` now asks
    directly, and fails with the old shortcut. The UI mirror now sends
    `corner` always: an optional field has to appear in the shared
    fixture, so a silent rename can't hide, and no polygon there has one.
  - **Tests:**
    - `a_corner_rounded_is_an_arc_between_its_sides`: on the circle to
      1e-3; an oversized radius held to half a side; one corner by
      itself; open ends, straight-through anchors and curved sides left
      alone; a rounded polygon drawn through the arcs.
    - `round_corners_rounds_each_shape_the_way_it_keeps_a_corner`: a
      rectangle, a star, a sharp brushed path and a smooth one at once,
      with widths carried and one undo; one anchor alone.
    - Sabotages that each fail one of them: the widths dropped, smooth
      paths rounded, the half-side clamp dropped, the handles pointing
      out of the corner, and the panel handing over no radius.
    - Smoke 9o25a: a pen zigzag's apex rounded by 20 and undone, and a
      polygon's corner slider.
- **The knife ✅** (`Session::knife(ids, a, b)`; the Knife tool, after
  Node on the rail, shift+A from the anchor tool). Drag a line across
  shapes and they fall into the pieces either side, Illustrator's and
  Affinity's knife.
  - **What it cuts:** every closed shape the line crosses, through
    `chitrakar_render::boolean`. Each half is the shape intersected with
    a quad reaching well past it on that side of the line. A half is
    split into pieces, each an outline plus the holes inside it: a ring
    is a hole if it sits inside an odd number of the others, and it
    belongs to the smallest outline round it. So a ring cut through its
    hole is two U shapes, and cut across its band is a strip and a piece
    that keeps the hole.
  - **What a piece is:** a copy of the shape (its look, effects and
    mask) with its piece's outline, in the shape's place. It is all one
    step, and the cut layer is in the shape's own group, with the line
    carried into that group's space. Curves become straight segments, as
    after any boolean; a brushed width is dropped, since its anchors are
    gone. Open paths are left to the scissors, and locked layers are
    left alone.
  - **What is asked:** what is picked, when anything is; otherwise every
    visible, unlocked shape on the page. While dragging, the line is drawn
    over the page, and shift holds it to an eighth of a turn.
  - **Found on the way:** the booleans could not combine two shapes
    wound opposite ways. Fragments are chained in the direction their
    rings run, so a reversed operand gave pieces that met head to head
    and never closed. Union, intersect and subtract all said `None`, and
    the editor said their edges overlapped exactly. That is any pen
    shape drawn anticlockwise against a rectangle. `boolean::oriented`
    now turns every ring of both operands, before they are cut up, so
    that what it covers is on the same side: an outline the way a
    rectangle runs, a hole the other way. Without it,
    `a_shape_wound_the_other_way_combines_the_same` fails.
  - **Tests:** `the_knife_cuts_a_shape_into_the_pieces_either_side`:
    - a rectangle cut down the middle, into two copies twenty across,
      with the page drawn bit for bit as it was;
    - one undo, and a miss that changes nothing and records no step;
    - the ring's two cases, with nothing picked;
    - a shape in a group turned a quarter, cut where the page line is;
    - an open path passed over, and a point that is no line refused.
    Taking the hole grouping away, or not carrying the line into the
    group's space, each fail it. Smoke 9o25b: shift+A, the line drawn
    while dragged, a rectangle cut and drawn where it was, undo and redo,
    and with a piece picked a line past it cutting nothing and one
    through it cutting only it. Ignoring what is picked fails it.
- **Lists ✅** (`TextSpec::list`: `None`, `Bullet` or `Numbered`;
  `TextSpec::markers`; *List* among the text settings). Canva's and
  Affinity's bulleted and numbered text: each paragraph an item, its
  marker in the margin, and its lines hanging beside it.
  - **Layout:** `list_marks` sets each item's marker in the style where
    its paragraph begins. Every line of the block is indented by the
    widest marker plus half an em, so numbers end in one column, and an
    item's text starts in one column whatever its alignment. The wrap
    width shrinks by that indent, and the block is that much wider. An
    empty paragraph is a gap between items, neither marked nor counted.
  - **One place for every consumer:** `set_line` adds the marker's
    glyphs just before an item's first line, after the spacing a
    justified line is given. So the raster draws markers, and `placed`
    hands them to the PDF, with no code of their own. A marker glyph is
    flagged (`Shaped::marker`): it stands for no byte of the block, says
    its own characters, and lies under no underline. `set` gives each
    line its x (`SetLine::x`, from `line_start`, which the SVG writer
    had been working out a second time) and its marker (`SetMarker`).
    The SVG writes the marker as a tspan of its own, on the item's
    first baseline, in the look of where the paragraph begins.
  - **Editing on the canvas:** a list is typed in a plain box, as a
    block with paragraph spacing is, since a textarea cannot draw markers.
  - **Fixture:** a quarter of the random blocks in lines are lists, from
    a stream of their own.
  - **Tests:**
    - `a_list_hangs_its_items_beside_their_markers`: the indent, one
      column of text, the bullet inked in the margin, "9." and "10."
      ending together, an empty paragraph skipped, a wrapped item hanging
      within the width, each bullet said once and a number a glyph at a
      time, no band under a bullet in a styled line, and a non-list
      untouched.
    - `a_list_leaves_as_the_engine_sets_it`: resvg inks marker and text
      within a pixel of the engine, for bullets and numbers.
    - `a_lists_numbers_are_in_the_pdfs_text`: Ghostscript reads back
      "1. Apples", "2. Pears", "3. Plums". The first attempt said the
      whole number on its first glyph and nothing on the second, and
      Ghostscript printed a control character for that.
    - Sabotages that each fail one of them: no indent at the line's
      start, no narrower wrap, bands over markers, no markers in the SVG,
      and the SVG's lines at the block's edge.
    - Smoke 9o25c: Bullets and Numbers keep the block's left edge and move
      the text in past the marks, and undo is one step.
- **Superscript and subscript ✅** (`StyleRun::baseline`, `shift`,
  `BASELINE_MOST`; *x²* and *x₂* beside the style toggles). Part of a
  block raised off its line or lowered, and smaller: x², H₂O, ™ and
  footnote marks.
  - **The model:** a run's `baseline` is how far it sits above the line,
    in its own size, negative to lower it, held to ±1 (serde default:
    on the line). Shaping moves the run's glyphs by that share of its
    size, and everything that reads shaped glyphs follows: the raster,
    `placed` and so the PDF, and the GPU's text.
  - **The line makes room:** `line_metrics` gives a raised run that much
    more ascent, which moves the baseline down, and a lowered one that
    much more below. A small superscript tucks under a line's own ascent
    and leaves the line as it was.
  - **SVG:** a run's shift is written as `baseline-shift`, which runs the
    same way up. A list item's marker takes the look of where its
    paragraph begins (`run_look`) but not its shift, because the engine
    leaves markers on the line. Written with the whole run, the bullet of
    a raised item came out raised in every reader, and the four SVG
    audits caught it on seed 36.
  - **The buttons:** each needs some text picked out. Superscript raises
    it 0.33 at three fifths of the block's size, and subscript lowers it
    0.2. Each reads as on over its own shift and takes both back off when
    pressed again. `runs.ts` knows the field, with the block's own value
    0.
  - **Editing on the canvas:** a block with a raised run is typed in a
    plain box, as one with a sized run is.
  - **Fixture:** a third of the random styled runs are raised or lowered,
    drawn after their size from the same stream.
  - **Tests:**
    - `a_raised_run_sits_above_the_line_and_the_line_makes_room`:
      measured from the baseline, since from the block's top a raised
      glyph has not moved — the baseline drops to make room. It inks
      higher raised and lower lowered, the line grows, nothing is cut
      off the raster, and a shift of nothing is none.
    - `a_raised_run_leaves_as_the_engine_sets_it`: resvg inks the
      shifted rows within one of the engine.
    - The run algebra's own case in `runs.test.mjs`.
    - Sabotages that each fail one of them: no room made, the shift the
      wrong way, no `baseline-shift`, and the field missing from the run
      keys.
    - Smoke 9o25d: disabled with nothing picked out, raised and read as
      on, back on the line, and lowered.
- **High pass ✅** (`Filter::HighPass { sigma }`; *Filter › High
  pass*, a *High pass radius* slider). The detail alone: what is under it,
  less its blur, on a grey of half. Over a copy of a photograph under
  overlay or soft light, it sharpens by that detail. That is the
  photographer's sharpening method, and the texture pass behind most
  "clarity" looks.
  - **The grey:** half is grey in the linear light the blends are worked
    in, which is what overlay leaves alone. It is 188 on screen, where
    Photoshop's 128 is grey in its gamma. The grey is premultiplied (half
    of what is covered), so a bare page stays bare. The result is held at
    nothing below, as a sharpen's is, and weighed in by opacity and mask
    as a blur is.
  - **The GPU** draws it with the blur's own six box passes and a flag on
    the pass that lays the blur back down (`fs_blur_down`'s `params.y`).
    A radius too small to move a pixel is drawn rather than skipped,
    because its answer is the grey by itself, not the page unchanged.
  - **Fixture:** half of the random sharpens are high passes, from a
    stream of their own.
  - **Tests:**
    - `a_high_pass_is_the_detail_on_a_grey_of_half`: flat ground and a
      flat middle are grey; inside an edge into light it is lighter and
      outside darker; half opacity is half way; no blur is the grey alone.
    - The GPU's blur test now draws two high passes, one of them at a
      radius of nothing, against the CPU. A grey of 0.4 in the shader, or
      the difference taken the wrong way round, each fail one of them.
    - Smoke 9o25e: the grey, a bare page left bare, an edge off the grey,
      and a wider radius reaching further. A slider that does not write
      the radius fails it.
- **Stroke what is picked ✅** (`Session::stroke_selection(color, width,
  align)`; *Select › Stroke what is picked…*, a window asking the width
  and the side). Photoshop's Edit › Stroke. As fill is here, it is made
  as artwork rather than paint: a shape layer with no fill and a line in
  the ink in hand, its colour and width numbers to change afterwards.
  - **Sides:** a path is stroked down the middle of its line
    (`stroke_align`), so a line to one side of the edge is a centred line
    along the edge moved half a width that way (`offset_outline`).
    Inside, the line's outer side runs along the edge, into the corners;
    outside, its inner side does, and its outer corners are round, as
    Photoshop's are. Across, it is the edge itself. A region too narrow
    to move inside falls back to the edge.
  - **Shared with fill:** what a region is as a path is now
    `region_outline`, which fill and stroke both use. A softened region
    is stroked along its outline.
  - **Tests:** `a_picked_region_is_stroked_on_the_side_asked_for`: nothing
    picked, no width and an unknown side are refused; each side inked
    where it lies and clear on the other side of the edge; the middle
    unfilled; the inside corner filled and the outside corner rounded;
    one undo each. The inward offset taken outward fails it. Smoke 9o25f:
    a marquee stroked inside in the ink, nothing outside, the middle
    clear, one undo; a window handing over the wrong side fails it.
- **Halftone ✅** (`Filter::Halftone { size, degrees }`,
  `halftone_cell`, `halftone_ink`; *Filter › Halftone* with *Dot size*
  and *Screen angle*). A printer's dot screen: a grid of cells, turned,
  each a black dot on white whose area is how dark the picture is under
  the cell's middle. Seen from far off it is the picture again; close up,
  it is the poster texture Canva's and Affinity's halftone effects give.
  - **Darkness as it looks:** darkness is one minus the luminance on the
    display's curve, so a mid grey on screen is half ink. A dot grows by
    area until it fills the circle the cell holds (π/4 of the cell).
    Darker than that, it grows on to the cell's corners, which it reaches
    at black, so black is solid ink. A dot under a pixel across fades
    with its size; without that, white wore a half-grey speck wherever a
    cell's middle was a pixel's. Its edge is a device pixel soft. What is
    not covered stays uncovered.
  - **The GPU** draws it among the filters of one pixel (kind 17) with
    one read more: the pixel the cell's middle falls on, taken off what
    is under it. That pixel is found through the inverse's own inverse,
    as the CPU works it, a quarter pixel in, so a middle on a pixel's
    edge is not settled by rounding. The region-render reach is half a
    cell's diagonal.
  - **Fixture:** half of the random pixelates are halftones, from a
    stream of their own.
  - **Tests:**
    - `a_halftone_is_dots_as_dark_as_the_picture`: black is all ink;
      white has none at cells of eight and of nine (nine puts a cell's
      middle on a pixel's); a mid grey is half ink on the display's
      scale, in black and white only; turned, the dots move; half
      opacity is half way, pixel by pixel; a bare page stays bare.
    - The GPU's one-pixel filters test draws two halftones, one turned,
      against the CPU.
    - Sabotages that each fail one of them: the GPU grid turned the other
      way, no fade for a sub-pixel dot (which the size-nine page alone
      catches), and darkness in linear light.
    - Smoke 9o25g: black and white only, about half ink, solid dots and
      clear paper, and a new dot size relaying them.
- **Emboss ✅** (`Filter::Emboss { degrees, height, amount }`,
  `emboss_offset`, `emboss_grey`; *Filter › Emboss* with *Light angle*,
  *Relief height* and *Relief amount*). What is under it pressed into
  grey metal and lit from one side: how light the picture is, taken as a
  height, and its slope toward the light. Photoshop's and GIMP's emboss,
  in grey.
  - **The relief:** each pixel is a grey of half less half of how much
    lighter the picture is `height` pixels toward the light than as far
    away from it, times `amount`, held between nothing and the pixel's
    cover. An edge that falls toward the light is lit and one that rises
    toward it is dark; flat ground is the half grey, in the light the
    blends are worked in, which overlay leaves alone. The light's
    direction is measured as a motion blur's is, from the page's x axis
    turning the way its y does, so 225 is the upper left (the preset).
  - **Whole pixels:** the two readings are a whole offset away. The
    offset is carried onto the surface by the view and rounded there
    once, so a relief is an edge a pixel wide, not a blur of one. The
    GPU is handed those two integers rather than working them out
    again, so neither renderer can round a half the other way.
  - **The GPU** draws it among the filters of one pixel (kind 18), with
    two reads more off what is under it, the edge read again past the
    edge. Inside a frame it goes back to the CPU, as every filter that
    reads a neighbour does. The region-render reach is the height.
  - **Fixture:** half of the random vignettes are reliefs, from a
    stream of their own.
  - **Tests:**
    - `an_emboss_lights_the_edges_that_face_the_light`: flat ground is
      the grey; the edge facing the light is white and the one facing
      away black; level with the light, an edge is flat; from the other
      side the two swap; no strength is the grey alone; half opacity is
      half way; a bare page stays bare.
    - The GPU's one-pixel filters test draws three reliefs (from above,
      from the right, and at 37°) against the CPU, worst and mean.
    - Sabotages that each fail one of them: the CPU's light turned the
      other way, and the shader's.
    - Smoke 9o25h: a white block on black is grey where flat, lit and
      shaded along its edges, and the light angle turns the edges round.
- **Twirl and pinch ✅** (`Filter::Twirl { degrees, radius, center }`,
  `Filter::Pinch { amount, radius, center }`, `warp_circle`,
  `twirl_source`, `pinch_source`; *Filter › Twirl* and *Filter › Pinch*,
  with the angle or amount, *Circle size*, and the centre across and
  down). The first filters that move the picture rather than recolour
  it: Photoshop's Distort › Twirl and Pinch.
  - **The circle:** both act inside a circle and leave the rest alone.
    Its middle is a share of the page's width and height, as a radial
    blur's is. Its radius is a share of half the page's shorter side, so
    one is the largest circle the page holds.
  - **Twirl:** the middle turns by `degrees` (clockwise on screen for a
    positive number), easing to nothing at the circle as the square of
    the way still to go. A point keeps its distance from the middle, so
    a centred disc stays a disc.
  - **Pinch:** a point `r` from the middle shows what was
    `R · (r / R)^e` from it, `e` one less half the amount. Positive
    pinches toward the middle and negative bulges out. The circle's edge
    stays put, so nothing jumps there. Three quarters of the amount was
    tried first and full strength shrank a disc to a speck.
  - **Reading back:** each pixel shows the picture where the warp says
    it came from, read between the four pixels round it, the edge read
    again past the edge. Out to the surface through the inverse's own
    inverse, as the halftone goes. A warp is weighed in covered or not,
    since it can bring the picture onto where nothing was.
  - **The GPU** draws them as filters of one pixel: kind 19, and kind
    26 (the next free number). The fragment's early return for an
    uncovered pixel lets these two through, and the four reads and
    their mixing are the CPU's lerp, not the sampler's. Inside a frame
    they go back to the CPU, as every filter that reads elsewhere does.
    The region-render reach is the circle's diameter.
  - **Fixture:** half of the random Gaussian blurs are a twirl or a
    pinch, from a stream of their own. Its constant was checked unique:
    the first two tried were already other streams' seeds.
  - **Tests:**
    - `a_twirl_turns_and_a_pinch_squeezes`: the corners stay; the edge
      down the middle swings clockwise, and the other way for a negative
      turn; a centred disc stays a disc; a square turned a half turn
      lands where nothing was; a pinch shrinks a centred disc and leaves
      its middle; a bulge swells it; half opacity is half way.
    - The GPU's one-pixel filters test draws two twirls and two pinches
      against the CPU, over the two bands and over the top band alone,
      where a twirl must reach the bare half.
    - Sabotages that each fail one of them: the CPU's turn reversed, the
      pinch's power inverted, uncovered pixels skipped on the CPU, the
      same skip in the shader, and the shader's turn reversed.
    - Smoke 9o25i: the corners stay and the edge swings clockwise, then
      back with the angle negative; a square in the middle is drawn in
      by a pinch and pushed out by a bulge.
- **Ripple ✅** (`Filter::Ripple { amplitude, wavelength }`,
  `ripple_wave`, `ripple_sine`, `ripple_source`; *Filter › Ripple* with
  *Ripple height* and *Wavelength*). Rippled glass, on the warp the
  twirl and pinch brought in: every point moved across by a sine of how
  far down it is and down by one of how far across, so an upright edge
  goes wavy down its length and a level one along its. Both numbers are
  in document pixels and anchored to the document.
  - **The same sine to both renderers:** the wave is the sine of the
    share of a wavelength past the last whole one, not of the distance
    itself. A point far out on a big page is then a small number to both
    renderers, rather than a large one each reduces its own way.
  - **The GPU** draws it as kind 27, in the twirl's and pinch's branch,
    and lets it through the early return for uncovered pixels as they
    are. The region-render reach is the amplitude.
  - **Fixture:** half of the random pinches are ripples, from a stream
    of their own.
  - **Tests:**
    - `a_ripple_makes_a_straight_edge_wavy`: no amplitude leaves the
      edge; a crest pulls the white across and a trough the black; away
      from the edge it is as it was; a level edge waves along its
      length; half opacity is half way.
    - The GPU's one-pixel filters test draws two ripples against the
      CPU, over the two bands and over one, where the ripple must reach
      the bare half by its amplitude.
    - Sabotages that each fail one of them: the CPU's waves driven by the
      wrong axis, the vertical wave's sign flipped, and the shader's axes
      swapped.
    - Smoke 9o25j: the edge goes wavy at a crest and a trough, the far
      sides stay, and no height straightens it.
- **Find edges ✅** (`Filter::FindEdges { amount }`, `edge_step`,
  `edge_white`; *Filter › Find edges* with *Edge strength*). The
  picture's edges drawn dark on white, Photoshop's Stylize › Find Edges.
  - **Channel by channel:** flat ground of any colour goes white; where
    a channel changes, that channel is taken down by how steeply. The
    steepness is a Sobel kernel over the eight pixels round, divided by
    four so a step from nothing to full is one, times the amount, held
    at nothing. A black edge is where every channel changes; a red
    square's edge is cyan.
  - **On the picture, not the screen:** the eight are read a document
    pixel away, rounded to whole device pixels from the view's scale and
    never less than one. So the lines keep their weight when the page is
    zoomed. The GPU is handed that step, rounded from the same
    `max_scale`, rather than working it out.
  - **The GPU** draws it as kind 28 among the filters of one pixel, the
    channels side by side. The region-render reach is the step's
    diagonal.
  - **Fixture:** half of the random high passes are edges, from a stream
    of their own.
  - **Tests:**
    - `finding_edges_draws_them_dark_on_white`: flat ground is white
      inside and out; the edge is dark and two pixels wide; a red
      square's edge is cyan; no strength is all white; drawn twice the
      size, the lines are four pixels wide and no wider.
    - The GPU's one-pixel filters test draws it weak and strong against
      the CPU, at the page's size and at two and a half times it.
    - Sabotages that each fail one of them: a fixed one-pixel step (the
      CPU's and the shader's), one channel read for another, and a
      kernel row read wrong in the shader.
    - Smoke 9o25k: flat ground white, the block's edge dark, and no
      strength white all over.
- **Colour overlay ✅** (`Effect::ColorOverlay { color, opacity }`,
  `overlay_tint`, `wear_overlay`; *Add effect › Colour overlay* with its
  colour and strength). The layer in one colour, Photoshop's Color
  Overlay: a picture, a shape or a group recoloured without touching
  what it is made of.
  - **Why it waited:** a first draft painted the colour over the layer
    after the layer had come down, as the inner shadow is painted. At a
    soft edge that leaves the layer's own colour under the new one, a
    ring of it all the way round. The draft sat in a stash for that.
  - **Recolouring, not painting over:** now it is the layer's own pixels,
    each premultiplied channel `w` of the way to the colour at the
    pixel's own cover, before they meet the page and before its other
    effects. Those are grown from the cover, which this keeps. A red
    disc overlaid in blue is the blue disc, pixel for pixel.
  - **Folded into one:** two overlays taken in turn are one toward the
    mix of the two, weighed by how much of each is left showing.
    `overlay_tint` folds them, so the GPU, which has one quad to carry
    them on, gets what taking them in turn gives.
  - **The GPU:** an overlay sends the layer to a surface of its own; a
    clone layer counts it as worn and goes aside. The quad that lays the
    surface back down carries the strength beside the blend and the
    colour in its three extra numbers. `fs_image` and `fs_blend` recolour
    what they read, and a strength of none is every other picture.
  - **SVG** writes it inside the layer's effect filter: the colour cut to
    the layer's alpha, weighed `w` against what came before at `1 - w` by
    `feComposite` arithmetic. That is premultiplied and in linear light,
    as a reader composites and as the engine recolours. The merge lays
    the recoloured source between what is under and over. The importer
    reads the chain back, in order, to the source.
  - **PDF** has no recolouring beside a live layer, so a layer in an
    overlay goes as the pixels the engine draws.
  - **One case it does not reach:** an upright frame holding something
    that reads the page is drawn where it stands rather than from its
    surface (`frame_in_place`), so an overlay on one is not applied. The
    GPU hands that page back already.
  - **Fixture:** a third of the random layers wearing effects wear an
    overlay too, from a stream of their own. Every cross-renderer, export
    and round-trip audit passed with them before anything else was
    written for it.
  - **Tests:**
    - `a_colour_overlay_recolours_the_layer_to_its_edge`: the blue disc
      edge and all, half strength half way, two in turn, a shadow it
      wears unchanged, and a group one colour where its layers overlap.
    - `a_colour_overlay_is_the_cpus` (GPU): alone, with a shadow, two at
      once, blended, faded and a group. Each is held to what the two
      renderers part by on the same page without the overlay, since an
      ellipse's edge is antialiased each its own way.
    - `a_colour_overlay_exported_comes_back_as_itself`: a reader draws
      blue on black with no red at the edge; a shadow and two overlays
      come back in turn; the page brought back is the page.
    - Sabotages that each fail one of them: recolouring without the
      pixel's cover (CPU and shader), folding two overlays wrongly, no
      call on the layer, no surface of its own on the GPU, the blended
      quad not recolouring, the SVG weights not adding to one, the SVG
      painted over as the draft was (red 123 at the edge), and the
      importer's order reversed. Two of these first passed, because blue
      has no red and the fold was checked in red and green only; the
      test now looks at the channels that can tell.
    - Smoke 8x2d: listed, the shape the overlay's colour all over with
      nothing outside changed, a new colour, half strength half way, one
      undo, and removed.
- **Oil paint ✅** (`Filter::OilPaint { radius }`, `oil_radius`,
  `oil_painted`; *Filter › Oil paint* with *Brush size*). The Kuwahara
  filter, Photoshop's Oil Paint in its plainest form: flat where a
  stroke would be, edged where the picture is.
  - **The squares:** round each pixel stand four squares, `radius`
    document pixels a side past it, one in each corner, overlapping on
    the pixel's own row and column. The pixel becomes the average of the
    square whose light, and whose cover, vary least. A square that
    straddles an edge varies most, so an edge is never averaged across:
    a sharp one stays exactly where it was. Stripes a pixel wide become
    their average, and a lone speck fades to its share of a square.
  - **Cover counts as well as light:** a first version measured light
    alone. Black and nothing are equally dark, so a square reaching past
    the page into the transparent margin of a view looked as smooth as
    one inside it, won by coming first, and the page's top and left
    edges went half covered on screen. The smoke block's screenshot
    showed it; a black square on a bare page now asks for it on both
    renderers, and the browser asserts the page's rim stays covered.
  - **Ties:** a square wins only by a margin (`OIL_MARGIN`, 10⁻⁴ in light
    squared), the first of them otherwise, so two equally smooth sides
    are not decided by rounding.
  - **The CPU** gets every square's sum from one picture of
    `(r+1)`-square sums, slid along the rows and then down the columns
    in double precision, so a wide brush costs what a narrow one does.
  - **The GPU** draws it as kind 29, walking the window once and adding
    each pixel to every square it is in. Past a radius of eight device
    pixels the page goes back, as a median's does.
  - **What the GPU cannot promise:** its surfaces carry a picture a
    thousandth or so off the CPU's floats, which moves a square's
    variance past the margin where two squares are nearly as smooth.
    There either answer is right, and the two renderers can take
    different ones. So its test over a mosaic holds the mean and how few
    pixels part, not the worst. Grain was tried first and is worse for
    the same reason: the two grains themselves part by a thousandth.
  - **Fixture:** half of the random medians are oil paints, from a
    stream of their own (the first constant tried was already a seed).
  - **Tests:**
    - `oil_paint_flattens_within_an_edge_and_keeps_the_edge`: the edge
      stays; stripes flatten; a speck fades and its neighbours keep the
      ground; half opacity is half way; a radius under half a pixel does
      nothing; a black square on nothing stays covered to its corners and
      nothing spills past it.
    - The GPU's one-pixel filters test: two bands at radii one, three
      and eight; nine handed back; a mosaic at one, two, five and eight;
      a black square on nothing.
    - Sabotages that each fail one of them: the roughest square chosen
      (CPU and shader), one corner for every square, the sliding window
      off by one, the centre row counted into the wrong squares, cover
      left out of the shader's measure, and the CPU's (in the browser
      too, where the rim went to 106 of 255).
    - Smoke 9o25l: a two-pixel line fades and a larger brush fades it
      more, a block's edge stays, and the page's rim stays black and
      covered.
- **Distances with alt ✅** (`app/src/measure.ts`: `gapsBetween`,
  `boxOf`, `union`; the Move tool's overlay, `measureAt`). With a layer
  picked and alt held, the Move tool shows how far it is from the layer
  under the pointer, and over nothing, from the frame it sits in or the
  page: a red line each, its length on a label in the person's units,
  and the box measured to outlined. Figma's and Illustrator's alt-hover
  redlines.
  - **What is measured:** side by side on an axis, the gap between them
    on it, drawn through the middle of the stretch they share. One wholly
    inside the other, how far in from each of the four sides. Overlapping
    without either holding the other, nothing on that axis. The first
    version took "inside on one axis" as inside; the unit test caught a
    narrower box below another showing how far in from its sides rather
    than only how far below, so it is whole nesting or nothing.
  - **When:** on every pointer move over the canvas with the Move tool,
    nothing pressed and alt held; pressing alt without moving shows them
    where the pointer last was, and letting go takes them away. What is
    picked, and anything travelling with it, is not something to measure
    to.
  - **Tests:**
    - `e2e/measure.test.mjs` (in `npm run test:unit`): twenty checks over
      side by side either way, above and below, apart on both axes,
      inside and holding, overlapping, touching, flush on three sides,
      and boxes from `bounds_of`.
    - Smoke 8x2e: none without alt; alt over the other layer is the gap
      between them; over nothing, the four distances to the page; and
      gone when alt is let go.
- **Lens correction ✅** (`Filter::LensCorrection { amount }`, `lens_k`,
  `lens_middle`, `lens_source`; *Filter › Lens correction* with
  *Distortion*). A wide lens's bowing taken out: what it bends outward
  (barrel) or inward (pincushion) made straight again, on the warp the
  twirl brought in. Photoshop's Lens Correction › Remove Distortion, in
  its one-number form.
  - **The bend:** a point a share `ρ` of the way from the page's middle
    to its corner shows what was `ρ · (1 − k ρ²)` along the same line, `k`
    a third of the amount (held to −1..=1). Positive draws the corners in
    to undo a barrel; negative pushes them out to undo a pincushion,
    where past the page's edge the edge is read again. The middle stays
    put.
  - **The GPU** draws it as kind 30 in the warps' branch. The region
    reach is the most a point moves: a third of the amount of the half
    diagonal.
  - **Fixture:** half of the random ripples are lens corrections, from a
    stream of their own.
  - **Tests:**
    - `a_lens_correction_bows_lines_about_the_middle`: a level line near
      the top bows up at its ends for a positive amount and down for a
      negative one, a dot at the middle stays, and none moves nothing.
    - The GPU's one-pixel filters test draws two among the warps and two
      over the mosaic. The two bands meet at the page's middle, which a
      lens barely moves, so the bend turned the wrong way in the shader
      only nudged the worst pixel there (0.059); over the mosaic it moves
      the mean to 0.147.
    - Sabotages that each fail one of them: the bend's sign flipped (CPU
      and shader), and the middle taken at the page's corner.
    - Smoke 9o25m: a level band bows up at its ends when the corners are
      drawn in, and down when they are pushed out.
- **Align to the page or a key layer ✅** (`Session::align_nodes_to`,
  `AlignTo { Selection, Page, Key }`, `prefs.alignTo`; the align bar's
  *Align to* row). With two or more layers picked, the align buttons line
  them up with each other as before, with the frame or page each sits in,
  or with a key layer. The key is the picked one wearing the handles: the
  others line up with its box, and it does not move, even when it is not
  among the layers being moved (locked, say). Illustrator's Align To
  Selection / Artboard / Key Object.
  - **Spacing evenly:** to the page, the outermost two go to its edges
    first, the first's near edge on the page's near edge and the last's
    far edge on its far edge, and the rest space evenly between. To a key
    it is as before, since the outermost two already stay put.
  - **One layer alone** still goes to what it sits in, whichever is
    chosen: it is its own key. So the choice is only offered from two.
  - **Remembered:** the choice is a way of working, so it is a
    preference, read back against its three values like the theme.
  - **Tests:**
    - `layers_align_to_the_page_or_to_a_key_layer`: two to the page's
      right edge and its middle; the same two to each other, where the
      page is not asked; spread over the page, the first and last at its
      edges and the middle between; the others to a key's left edge with
      the key unmoved; and a key outside the moved set still holding
      them.
    - Sabotages that each fail it: Page falling back to the selection's
      box, Key doing the same, and the page spread skipping the edges.
    - Smoke 8x2f: the choice offered with two picked; to the page, both
      at its right edge; to the key, the key unmoved and the other's
      edge on its edge; to each other, the rightmost; and the choice
      written to the preferences.
- **Find and replace ✅** (`app/src/findReplace.ts`: `matchesIn`,
  `replaceIn`; *Edit › Find and replace…*, Ctrl+F;
  `Session::apply_json_as`). Words looked for across every text layer
  and put right in all of them at once. The window counts as the words
  are typed, "4 found in 2 layers", with *Match case* and *Whole words*,
  and *Replace all* is one step of history under its own name,
  "Replace “fish” with “cat”".
  - **Styling kept:** a block's styling is byte ranges over its text, and
    every replacement moves the ranges after it. Each is carried through
    `shiftRuns` one replacement at a time — the arithmetic a keystroke
    already goes through — so a bold word after the replacements is
    still that word, a bold word replaced is the replacement in bold, and
    a run over only what was taken away goes with it.
  - **Finding:** the words literally (a dot is a dot), never overlapping;
    case-insensitive by the pattern's own flag rather than by lowering
    both sides, which can change a string's length; a whole word is not
    beside a letter, a digit or an underscore of any script, so `café`
    is a word.
  - **The label:** a batch of edits to several layers is named "Edit 2
    layers" by the history, which says nothing about what was done, so a
    command can now arrive with a label of the caller's own
    (`apply_json_as`, wasm `apply_as`).
  - **Tests:**
    - `e2e/findReplace.test.mjs` (in `npm run test:unit`): sixteen checks
      over case, whole words, accents, literal dots, overlap, growing
      replacements, and runs before, on and after what is replaced,
      past accents and emoji, and over what is taken away.
    - Sabotages that each fail it: the runs not carried, and the running
      shift between replacements dropped ("two fcat").
    - Smoke 8x2g: two text layers; counted as typed and again with case
      minded; both changed with the capital left alone; one step of
      history under its own name; one undo puts both back.
- **Styles that set type ✅** (`Look::text`, `TextLook::of` / `give`).
  A look taken off a block of text now says how it is set — face, size,
  weight, slant, underline, strike, alignment, line height, letter and
  paragraph spacing, list — as well as what it is painted with. So a
  style kept off a heading is a paragraph style. Given to another block
  it sets that block the same way, and the block's words, the styling of
  parts of them, its width and its path stay its own. Copy style and
  paste style carry it the same way.
  - **Only where it means something:** a shape given such a style takes
    its paint and changes no shape. A look taken off a shape has no type
    in it, so it changes no text's type. The field is left out of the
    file when absent: a document that never kept one writes what it
    always wrote, and a look written before this reads as one that says
    nothing about type.
  - **The chip** of a style that sets type shows its name in that weight
    and slant, and its title says what it sets ("set text in 90px
    bold").
  - **The wire's mirror:** the TypeScript side declares `Look.text` and
    `TextLook`, so the shared commands' kept style ("cool") now carries
    type, and the audit that holds the UI's vocabulary to serde's sees
    every name.
  - **Tests:**
    - `a_style_kept_off_text_sets_other_text_the_same_way`: kept as it is
      set; another block set the same way and painted the same, with its
      words, runs and width its own; a shape keeps its shape; one undo;
      a shape's look carries no type and leaves a block's type alone; a
      look written before reads.
    - Sabotages that each fail it: no type read off a text layer, and the
      size not given.
    - Smoke 8x2h: a block set at 90 and bold, kept; the chip bold; the
      other block given it goes from 48 to 90 and bold, its words its
      own; one undo. (The first draft set 48, which was already the
      default, so that check proved nothing until the number changed.)
- **Styles that are followed ✅** (`Node::style`,
  `Command::SetStyleLink`, `Session::followers_of`; the panel's *Follows
  the style … Detach* row and the ringed chip). A style was copied onto
  a layer and forgotten, so changing it changed nothing already given
  it. Now a layer given a style, or the layer one was kept from, follows
  it. Keep the style again under the same name (shift-click its chip)
  and every other follower takes the new look in the same step — "Keep
  the style button, and 2 layers following it" — which one undo takes
  back. Affinity's and Figma's linked styles: change the button once and
  every button changes.
  - **Letting go:** forgetting a style lets its followers go with their
    look kept. *Detach* in the panel lets one go: it keeps the look and
    stops following. Edits to a follower's own look are its own until
    the style is kept again, which gives it the style's look — what
    following means.
  - **In the file:** which style a layer follows is in the file and comes
    back; left out when a layer follows none, so a document that never
    linked one writes what it always wrote.
  - **A new command:** `SetStyleLink { id, style }`, with its inverse,
    its slot (`style`), no repaint (the look comes with commands of its
    own), and a label ("b follows button", "Detach b from its style").
    The fixture names it and holds two of it, link and unlink, and its
    document has a layer following "warm", so every audit — commands,
    the wire and its TypeScript mirror, the container's inventory — is
    asked of one.
  - **Tests:**
    - `layers_given_a_style_follow_it_when_it_is_kept_again`: the layer
      kept from follows; layers given it follow; kept again, both take
      the new look in one step under its label, and one undo takes it
      back; a detached layer is left alone; the links survive save and
      load; forgotten, the followers follow nothing and keep the look.
    - Sabotages that each fail it: followers not given the new look, no
      link on giving a style, and forgetting leaving the links.
    - Smoke 8x2i: kept, given, the chip ringed; the first repainted and
      the style kept again turns the other too; detached, it stays as
      the style moves on.
    - `a_style_kept_by_name_gives_its_look_to_another_layer` said the
      old behaviour — a layer given the style kept its old look when the
      style was kept again — and now says the new one: it follows to the
      newer look, and keeps that when the style is forgotten. The
      fixture's unlink first named a layer following nothing, which the
      audit that applies each command alone rightly called a command
      that proves nothing; it unlinks the layer following "warm" now.
- **A phone's own bar** (Phase 4, the first of the mobile work): below
  640 wide or 500 tall the menus are a drawer behind one Menus button,
  the bar one row, the rulers gone, and the rail held sideways scrolls.
  - **Tests:** smoke 11a at 390×844 and 844×390 with touch: no
    menubar, a bar under 60 tall, no rulers; the drawer opens, Edit ›
    Find and replace closes it and opens the dialog, the scrim closes
    it; sideways, the rail scrolls and Zoom is reachable. 9s and 9u go
    through the drawer below the menubar's sizes, and 9u finds the menu
    that overflows rather than naming one.
- **A hand on glass** (Phase 4): two- and three-finger taps undo and
  redo, a long press opens the canvas's menu, a pen rejects the palm,
  and a touch screen holds Shift, Alt and Ctrl on screen.
  - **Tests:** `e2e/touch.test.mjs` (in `npm run test:unit`): taps
    counted when the last finger lifts, staggered or not; spoiled by
    travel, by slowness, by a cancelled finger; held keys worn on an
    event (control off a Mac, command on one), latched by a tap and let
    go by a hold. Smoke 11b on a 1024×768 touch page: no keys on the
    mouse's page; two fingers undo a rect and three bring it back, while
    a pinch and a hold change nothing; a long press opens the menu and
    lifting draws nothing, a finger that moved on opens nothing; palm
    then pen draws one rect and the pen's, a finger with the pen down
    draws nothing, and with it lifted a finger draws again; Shift tapped
    adds to the layers picked, tapped again picks one, held lets go
    with the finger. Shift held on the screen with two layers picked
    asks that the panel scrolls, which it did not.
  - The shell: `cargo clippy --target aarch64-linux-android -- -D
    warnings` clean against NDK r27c; CI jobs `shell-android` and
    `shell-ios` keep it so.
- **Share, and Android's addresses** (Phase 4): Share… in the export
  window where the device can; the shell opens and writes `content://`.
  - **Tests:** smoke 11c with the sheet stood in for: no Share without
    one; one PNG shared and the window closed; put away, the window
    stays; the set is three files in one sheet, @1x @2x @3x; a PDF the
    sheet will not take is refused, saying so. `e2e/files.test.mjs`:
    names and folders of paths, Windows paths and content addresses,
    encoded, numbered and not encoded at all. Shell:
    `a_content_address_is_kept_as_it_came_and_a_path_is_a_path`.
- **The layers as a sheet** (Phase 4): below 640 wide the panel is a
  bottom sheet, half or nearly all of the screen, moved by its grip.
  - **Tests:** smoke 11d at 390×844 with touch: the sheet's bottom is
    the screen's and its width the screen's, its top near the middle;
    pulled up it is tall, down it is half, a short pull leaves it, a tap
    toggles, pulled down from half it goes and the bar's button says so;
    at 844×390 the side overlay, no grip.
- **A phone's browser** (Phase 4): `100dvh`, no pull-to-refresh,
  `viewport-fit=cover` with the editor clear of the notch, a web app
  manifest and the bar's colour following the theme.
  - **Tests:** smoke 11e at 390×844 with touch: the viewport covers;
    overscroll is none; the editor is the window's height and the rail
    on it; the manifest says standalone and Chitrakar, its icons decode
    at the sizes it gives; the theme-color meta is the bar's colour.
- **Offline** (Phase 4): a service worker keeps what the page loads.
  - **Tests:** smoke 11f on a phone page: the page, its script, the
    engine and the fonts are kept; offline, the reload opens the editor
    and a rect drawn is a layer. With the worker's fallback taken out,
    the offline reload never shows the canvas.
- **Pictures given to the app** (Phase 4): the manifest's share target
  and file handlers.
  - **Tests:** smoke 11g: the share form posted to share-target opens
    the page with the picture placed by name, the address plain again,
    the worker's store emptied, and a reload placing nothing; a picture
    handed over through the launch queue is placed.
- **The phone rail's options, and the keyboard** (Phase 4): the palette
  and a paint tool's controls lie in a row on a phone, and picking the
  tool scrolls them into sight; `interactive-widget=resizes-content`.
  - **Tests:** smoke 11h at 390×844: with Paint picked the rail is under
    72 tall, the width and softness are on screen inside it, and the
    palette's swatches are one row. 11e: the viewport asks the keyboard
    to resize the page.
- **One finger, with a pen** (Phase 4): `prefs.finger` — auto (draws
  until a pen is used, then moves the page), draw, or pan.
  - **Tests:** smoke 11i on a 1024×768 touch page: a finger draws
    before any pen; the pen draws; after it a finger draws nothing and
    moves the page by exactly its drag; set to draw in Preferences it
    draws again; a page opened with `finger: "pan"` never draws.
- **Equal spacing ✅** (`app/src/spacing.ts`: `equalSpacing`; the Move
  tool's drag). Figma's spacing guides: a layer carried past a
  neighbour catches the gap that neighbour leaves to the one before it,
  carried between two it catches the middle, and the equal gaps are
  marked while it is held there — the alt distances' marks, in pink,
  since they say something else: these gaps are the same. Only what is
  beside it counts, a row for spacing along x and a column along y. It
  comes after the edges and middles: an axis that has caught a line is
  not moved off it, though its gaps are still marked when they happen to
  be equal there. Ctrl drags free of it, as of every other line.
  - **Tests:**
    - `e2e/spacing.test.mjs` (in `npm run test:unit`): past the second
      of two, before the first, between two (the middle: 35 a side,
      which the draft of this test had wrong at 30), down a column; not
      another row, not one neighbour alone; already in place, it stays
      and says so.
    - Smoke 9cb: squares at 20, 100 and 200; the third dragged 18 left
      shows two 40s while held, lands at 180, and the marks go on
      release; with ctrl it lands where the hand put it. With the
      correction left out of the drag, it lands at 182 and the block
      fails.
- **Clipping and effects, asked what is really declined** (the GPU
  backend). The fixture audit printed what the backend hands back —
  `SetClipped`, `SetEffects`, `TurnCanvas`, `StraightenCanvas`, `SetMask`
  — and a throwaway test took each page apart a layer at a time to find
  why. Two came off, and the reasons were three defects:
  - **What a clipped layer is held to read the base whole.** The page
    holds a layer to its base *before* the base's effects are drawn; the
    backend and the exporters' `clip_pixels` read the base drawn whole,
    shadow and all, so a layer held to a base casting a shadow showed
    through the shadow in an SVG, and the backend declined the page.
    `clip_coverage_at` draws the base the way the page captures it and
    reads back that same cut; both read it now. A base wearing effects
    is drawn, except a frame (drawn by a road of its own).
  - **A held layer's effects escaped its base, on the page.** Cut before
    the effect was grown, and then written inside the base's *window* —
    grown by the effect's reach — rather than its alpha: a shadow held to
    a disc filled the disc's box's corners. Cut by the base's alpha now
    (`stamp`'s `held`, on the two roads that draw a held layer's
    effects). That settled the "soft clip" question the backend had been
    declining over (see the September note): drawn by both now.
  - **The SVG let them escape too**, on purpose — its comment had written
    down what the renderer happened to do. A held layer with an effect
    that reaches out (a drop shadow, an outline) wears its base's mask
    twice now: inside the filter, so the shadow grows from what shows,
    and outside it, so it shows only where the base does.
  - **Tests:** `a_shadow_held_to_a_disc_stays_inside_the_disc` (failed
    before: a solid shadow outside the disc), `a_clip_for_an_exporter_
    leaves_the_bases_shadow_out` (fails with the base read whole);
    backend `a_layer_held_to_a_base_with_a_shadow_is_held_to_the_base`,
    `a_held_layer_casting_a_shadow_past_its_base_is_cut_by_the_base`,
    `a_held_layer_wearing_an_effect_on_a_turned_base_is_drawn`. Random
    SVG page 223 (a rect held to brush strokes, casting a shadow) is what
    caught the exporter; page 160 (a frame's inner shadow) is why only
    effects that reach out are cut twice. The backend's audit now draws
    58 of the fixture's commands and declines 3: turning and straightening
    the canvas, and a mask on a frame.
- **A turned frame, drawn by the GPU backend.** Turned a quarter or
  straightened, a canvas with a frame on it went back whole: the frame's
  arm refused anything not upright, since everything inside a frame is
  bounded by a rectangle of whole pixels and a turned frame's edge is
  not one. It goes on a surface of its own now, as the CPU draws it —
  the ground over the frame's whole box, what it holds drawn whole —
  and the surface is cut on the way down by how much of each pixel the
  frame covers, read from the reference renderer itself
  (`chitrakar_render::frame_cover`, its `rect_coverage` over a region),
  riding the same coverage texture as a mask (`Held::turned`;
  `mask_texture` takes a `Held` now rather than its four parts). Faded
  and blended it agrees. Wearing an effect, the cut belongs inside the
  surface the effect is grown from — the frame's silhouette is the cut
  frame — on draws that each carry a mask of their own; so what the
  frame draws goes on a surface inside that one, and the cut is taken
  on that inner surface's one quad. Held to the layer under it, or
  wearing an effect inside another frame, it still goes back.
  - **Tests:** `a_turned_frame_is_cut_the_way_the_cpu_cuts_it` (plain,
    faded, Multiply; with the cut left off the way down, the plain page
    is a tenth apart on average and the block fails);
    `a_frame_paints_its_ground_and_keeps_its_contents_inside` asks the
    quarter-turned frame to be drawn and to agree, where it pinned the
    refusal; and the same page casting a shadow (with the inner
    surface's cut left off, a tenth apart). The fixture audit draws 60
    of its commands now and declines 1: a mask on a frame that wears an
    effect, which is the open question recorded with seed 1233 (what
    lies past the page casting effects onto it). The half-turn that no
    single layer explained was this frame too — drawn now.
- **The layer list under a finger** (Phase 4): a swipe scrolls, a long
  press lifts a row to be carried.
  - **Tests:** smoke 11j on a 1024×768 touch page with 24 layers,
    through `Input.dispatchTouchEvent`: a swipe on the rows scrolls the
    list and reorders nothing; held half a second, the top row is
    carried to third, and nothing scrolls. Before the change the same
    swipe left the list where it was.
- **The shared fixture, an eighth time: a colour overlay, by name.** The
  one effect that changes a layer's own pixels rather than drawing
  beside them had only ever been asked of pages nobody wrote. It stands
  on the photograph now, in the palette's ink by name. One audit stopped
  holding, and it was the right one: the file format's inventory of
  fields a `.chitra` cannot be missing grew by the overlay's eight.
  Everything else held, so the code it was meant to exercise was broken
  to see who would notice — and where it first went says why that step
  is not optional. It went on the lettering first, and there nothing
  noticed the GPU backend ignoring overlays: the text wears a bevel at
  0.85, which re-lights nearly every pixel of its thin strokes, so the
  overlay under it hardly shows — to the audit or to the eye. On the
  photograph:
  - the palette walk skipping the overlay's colour: caught by
    `a_palette_change_leaves_nothing_meaning_the_old_colour`, and only
    because the overlay now stands in the document;
  - the GPU backend ignoring overlays: caught by the shared document's
    own cross-renderer audit (after `MoveNode`, which brings the
    photograph up), besides `a_colour_overlay_is_the_cpus` and the pages
    nobody wrote;
  - the SVG export dropping it: caught by the importer's round trips.
- **A photograph's page in a phone's memory** (Phase 4): the first frame
  waits for the view; 8-bit pictures are widened in place; saving
  borrows the document.
  - **Tests:** smoke 11k (a 4032 × 3024 page on a phone page costs
    under 60 MB of the engine's memory, and is fitted to the screen);
    `an_eight_bit_picture_is_widened_in_place_to_what_the_general_road_
    gives` (grey, grey and alpha, RGB, RGBA as PNG; grey and RGB as JPEG;
    fails with grey-and-alpha's alpha dropped); `tests/peak.rs`.
- **A shrunk picture is drawn from a pyramid** (Phase 4): levels made
  once and kept, read where a device pixel covers two texels or more —
  a fitted 12 MP photograph's frame from 80 ms to 21, both renderers.
  - **Tests:** `each_level_is_the_last_averaged_two_by_two` (odd edges
    average only what is there); `a_pyramid_is_made_once_and_kept`
    (and another picture under the same id gets its own);
    `a_fine_checker_shrunk_a_long_way_is_flat_grey` (fails drawn from
    the picture itself, a ninth spread 0.485–0.515); the GPU's
    `a_shrunk_picture_is_averaged_the_way_the_cpu_averages_it` gained an
    odd size and a tenth (fails with the span left out, or with the
    picture itself uploaded instead of the level);
    `a_picture_at_half_its_size_is_each_two_texels_averaged` holds,
    now through the half level.
- **A picture in perspective drawn from its pyramid** (Phase 4): a
  level a pixel, mixed between two; the inside of the quad a row at a
  time — 450 ms to 92 for a 12 MP photograph with its corners moved.
  - **Tests:** `a_checker_in_deep_perspective_is_grey_from_end_to_end`
    (shrunk by four to forty; fails drawn from the photograph itself,
    0.066 off); `corners_moved_to_a_shrink_draw_the_shrink` (at a half,
    a quarter, an eighth; fails with the level's coordinates left
    unscaled); `a_picture_going_away_blurs_without_a_seam` (the
    stripes' contrast row by row moves under 0.04, and 0.09 with the
    levels read one or the other). `a_picture_goes_where_its_corners_
    are_put` holds as it was.
- **A filter inside a frame, on the GPU** (Phase 4): the sixteen
  filters that read a neighbourhood were all handed back inside a frame,
  since their passes read as far as the page goes and the CPU stops at
  the frame. Each draw now carries the frame's rectangle
  (`Item::within`), its quads are drawn over it, and its passes are
  bound a copy of the page's numbers with the frame's edges in place of
  the page's — so every reading that stopped at the page stops at the
  frame. Three readings had never stopped at the page at all, clamping
  to the texture instead: the warps (twirl, pinch, ripple, lens), the
  halftone's cell and the emboss's offset now read through `texel_at`,
  which clamps to those edges. A masked frame wearing an effect is drawn
  too: it was handed back for want of knowing, and asked directly it
  agrees on and off the page's edges. Drawing filters in frames turned
  up an older defect: a frame wearing an effect while holding something
  that works on the page — an invert, a noise — is drawn in place by the
  CPU (`frame_in_place`) and was put on a surface of its own here, where
  there was nothing under it to work on: a tenth of full scale apart,
  on a page whose halftone had kept it out of every audit. It goes back.
  The random pages drawn went from 1029 to 1036 of 2000.
  - **Tests:** `a_filter_inside_a_frame_reads_only_the_frame` (seventeen
    filters, stripes against the frame's white ground; with the frame's
    edges left off the passes, thirteen of them come apart, and with
    `texel_at` clamping to the texture, six);
    `a_masked_frame_wearing_an_effect_is_drawn_the_way_the_cpu_draws_it`
    (a shadow, an outline, an inner shadow, on and off two edges, faded
    and in a faded group, at two views; with the frame's mask left off
    its silhouette, 0.04 apart); `a_frame_between_pixels_is_drawn_and_
    what_it_cannot_hold_goes_back` draws the pixelate at a zoom now and
    asks the in-place frame with an inner shadow to go back.
- **The frames a phone draws most** (Phase 4): a 12-megapixel
  photograph fitted to a 1170 × 878 view, timed natively through
  `Session::render_cached`, before → after:

  | Gesture | Before | After |
  |---|---|---|
  | Drag the photograph | 44 ms | 11 ms |
  | Drag a curves layer's opacity | 204 ms | 39 ms |
  | Drag a shape under a curves layer | 27 ms | 6 ms |
  | Drag a blur layer's opacity (curves under it) | 269 ms | 77 ms |
  | Pan the view | 316 ms | 99 ms → 71 ms |

  Two changes. A curves layer crossed each channel into the display
  encoding and back with nine powers a pixel, three of them the same
  number every time; the whole crossing is one table a channel now,
  worked out exactly at 4096 steps once a pass (`curves_through`). And
  an upright picture read one tap a pixel — a photograph at any zoom —
  is drawn by rows and columns: where a column reads, between which two
  texels and how far, and how much of it the outline covers, are worked
  out once a column, and the same for a row, so a pixel is four fetches
  and its blend. The same arithmetic in the same order, to the bit.
  - **Tests:** `a_curves_layer_crosses_by_table_where_the_power_law_
    would` (within 2e-5 from the deepest shadow to five times white;
    fails at 0.014 with the table sixteen times coarser, and above white
    with the gain left off); `an_upright_picture_is_the_same_picture_by_
    rows_and_columns` (nine pages, enlarged, shrunk onto the half and the
    quarter, between pixels, flipped both ways, faded, masked, blended,
    bit for bit against the other road; fails on the first page with
    the row weight nudged by a thousandth).
- **A blur down the columns, a row at a time** (Phase 4): the box
  passes walked a column to its end and then the next, reading a pixel
  a row away — eighteen kilobytes on a phone's view — at every step, and
  that was most of what a blur cost. Every column's running sum now
  moves down together and each row is read in order, once; the rows the
  window still reads are kept in a ring of `radius + 1`. The same for a
  feathered mask's plane. Panning the fitted 12 MP photograph under a
  blur layer went from 69 ms a frame to 39, and under a blur and a
  curves layer from 99 to 71.
  - **Tests:** `down_the_columns_a_row_at_a_time_is_the_same_blur` and
    `a_plane_blurred_a_row_at_a_time_is_the_same_plane`, bit for bit
    against the column-at-a-time walk over three regions, five radii and
    both edges; each fails with the ring one row short.
- **The browser's frame** (Phase 4): every number above was measured
  natively, and the browser is where the app runs. Measured there — the
  engine's own WebAssembly in headless Chromium, warmed up, the fastest
  of twelve frames — a full repaint of the fitted 12 MP photograph was
  170 ms, against 13 natively. WebAssembly has no instruction for a
  power or for rounding half away from zero, so `powf` and `round` are
  routines in software there, and the frame crossed the sRGB curve with
  three of each a pixel on its way to the screen, and an adjustment
  crossed it again. Three changes, none of them changing a native
  answer:
  - **The frame's eight bits by table** (`linear_to_srgb8`, `Srgb8`): the
    answer is one of 256, and the curve only climbs, so it is how many
    of the 255 points where it steps up a value has reached — found once
    by bisection over the floats with the curve as it is, and indexed by
    a value's leading bits finely enough that a channel is one
    comparison. The same byte as the curve's own for every float from
    nothing to one (checked once over all 1.07 billion of them, and over
    every 331st in the suite). Alpha rounds through `round_unsigned`,
    `round` to the bit without the call, and an opaque pixel skips its
    three divisions by one (`a_pixel_encoded_is_the_formulas_bytes`).
  - **The curve's two powers by table in WebAssembly** (`power`,
    `PowerTable`): 4096 points an octave, within a part in ten million
    of `powf`; natively `powf` stays the reference. A curves layer's
    tables are built through the transfer tables rather than twelve
    thousand powers a frame.
  - **No power where none is needed**: an exposure's gain once a pass
    (`Prepared::Gain`), and levels at a gamma of one take no power.

  | In Chromium, best of 12 | Before | After |
  |---|---|---|
  | Repaint the fitted photograph | 170 ms | 37 ms |
  | …under a curves layer | 184 ms | 56 ms |
  | …under a curves and a blur layer | 226 ms | 98 ms |
  - **Tests:** `eight_bits_by_table_are_the_curves_own_eight_bits` (every
    step and its neighbours, three million values across nothing to one,
    the ends; and no two steps between neighbouring starting points);
    `the_curve_by_table_is_the_curve_by_powers` (both powers, relative
    error under 1.2e-7 everywhere the table answers — a thousandth of an
    octave was tried first and fails at 5e-7). The browser suite runs the
    WebAssembly engine, so it is what holds the tables at pixel level.
- **Every layer, timed in the browser** (Phase 4): each adjustment and
  filter laid over the fitted 12 MP photograph in Chromium, what it adds
  to a full repaint. The slow ones were slow for WebAssembly's reasons,
  not their own: besides the power, `f32::max` and `f32::min` are calls
  there (the machine's own maximum hands a NaN on, Rust's hands the other
  number back), and so is `round`. The adjustments and the surface blur
  say `hi`, `lo` and `rounded` now (`HiLo`: the same answers, as
  comparisons and a truncation), the motion blur and the clone round
  the same way, and levels raise to a gamma through a table of that
  power kept while the gamma stays (`chitrakar_color::pow`, within a part
  in a million; natively `powf`). Added to a repaint, before → after:
  levels at a gamma of 1.3, 150 → 21 ms; posterize 96 → 36; selective
  HSL 98 → 47; white balance 39 → 1; motion blur 95 → 69; invert 50 →
  30. **Still slow:** a radial blur, 3.7 s — up to 129 samples a pixel
  at a zoom that reaches the page's corners, which is the algorithm and
  not the platform; the surface blur (141 ms), the median (106), the
  pinch and the twirl (~95, a sine and cosine a pixel).
  - **Tests:** `round_half_away` is `round` to the bit either side of
    nothing across the range and its ends (in
    `eight_bits_by_table_are_the_curves_own_eight_bits`);
    `a_power_by_table_is_the_power` (seven exponents from a gamma of 20
    to a tenth, under 1e-6 relatively, a denormal answer within 1e-30,
    and the table kept while the exponent stays).
- **A view moved keeps its surfaces; the pinch and the surface blur in
  the browser** (Phase 4). `set_viewport` let go of the frame's surface
  and its scratch, so every frame of a pan asked for sixteen megabytes
  of zeroed memory and zeroed it — the browser's profile had it at a few
  milliseconds a frame; a whole repaint clears what it draws over, so
  they are kept now and only a view of another size gets new ones. The
  pinch raises to its power through `chitrakar_color::pow`, and the
  surface blur's likeness folds with `hi` rather than `f32::max` (a call
  in WebAssembly).
  - **Tests:** `a_view_moved_shows_what_a_fresh_view_there_shows`
    (panned, zoomed in, zoomed out past the page, panned again, with a
    shadow and a blur filter on the page: bit for bit what a session
    that only looked there shows).
- **A pan in the app** (Phase 4): measured in the built app in Chromium
  — a page of 200 rounded rectangles, thirty wheel steps, each timed to
  the second animation frame after it (about 33 ms of that is the two
  frames) — a step went from 99 ms to 76. Two things. A view moving
  called the app's whole `refresh`, which read the layers, guides,
  palette, styles, region, kept regions and comps back as JSON and handed
  them all to React, and bumped the save counter, every frame of a pan:
  the view effect calls `present` now, the frame alone, since a view
  changes none of those. And a rounded rectangle's edge asked `hypot` —
  a routine in software in WebAssembly — at every sample, sixteen by
  sixteen a corner pixel: along a side one of the two lengths is nothing
  and `hypot` of a length and nothing is the length, to the bit, so only
  the corners ask now, and in WebAssembly they take the root of the sum
  of the squares (`length`; natively `hypot` still).
  - **Tests:** `a_rounded_rectangles_distance_is_the_same_without_the_
    call` (four rectangles, a grid round each, inside and out, bit for
    bit against the formula that always called `hypot`); the browser
    suite for the view effect.
- **Opening a photograph in the browser** (Phase 4): a 12 MP JPEG, timed
  in Chromium from its bytes to its first frame. The import's colour
  normalization said it skipped a profile that was already sRGB and
  never asked: every picture with sRGB written into it — a browser's own
  encoder, many cameras — went through a transform and a copy of all
  its pixels for an answer that was the same picture
  (`is_srgb_already`: a matrix and three curves, the primaries and white
  within a two-thousandth, each curve within a ten-thousandth at a
  thousand points). A picture that does need it — a phone's Display P3,
  most of them — is carried across a 64 KB piece at a time rather than
  through a copy of the whole of it, 48 MB less at the moment a
  12-megapixel one opens. And the first level of its pyramid is read two
  rows at a time as slices, an opaque texel without its multiplications
  by one, the levels rounded without `round`'s call. The JPEG's decode
  went from 300–550 ms to 195–310; the pyramid is about 250 ms of the
  first frame in the browser and a hundred natively, which is most of
  what is left, once.
  - **Tests:** `srgb_is_left_alone_and_p3_is_converted_in_pieces` (sRGB
    bytes untouched; P3 over seventy thousand pixels in pieces is what
    the whole at once gives); `the_quick_two_by_two_is_the_loops_two_by_
    two` (an odd-sized picture's first level, bit for bit against the
    loop every texel used to take).
- **Text shaped and rasterized once** (Phase 4): a page of forty
  paragraphs was shaped from nothing on every frame — the shaper working
  out its plan for the face afresh each time, a billion of five billion
  instructions — and every block rasterized whole even where only a
  corner of it was being repainted. What a face makes of a string is
  the same at every size and zoom, so the shaper's answer is kept by the
  face and the string (`text::shaped`, let go of wholesale past 4096);
  and a block's raster is kept by what the block says, the scale it is
  seen at and where its origin falls between the pixels
  (`text_raster_kept`, the last 48 and no more than 48 MB). The row pass
  of the box blur reads its middle without asking where the line ends.
  Natively, the page of paragraphs repaints in 26 ms rather than 37, and
  dragging a shape across it in 1 ms a frame rather than 16.
  - **Tests:** `text_kept_from_one_frame_is_never_another_frames_text`
    (a block drawn, changed in its words, its size, its scale and its
    place between pixels, and back: each time the raster kept is the
    raster made; with the place between pixels left out of the key it
    fails at that step); `a_blur_in_three_runs_is_the_same_blur` (bit
    for bit against the pass that asks at every read).
- ✅ **Draft frames while a slow view moves.** A page whose last full frame took longer than 70 ms — a radial blur, a dozen filters, a phone — is drawn at half the resolution while the view is moving, which is a quarter of the pixels, stretched over the canvas, and at its own 160 ms after the view keeps still. The GPU path never drafts; a quick page never drafts. Smoke 11l sets the threshold below nothing so every page drafts, and checks the draft is the same page and the settled frame is the same to the byte.
- ✅ **Radial blur a run at a time.** The radial blur took each pixel's samples one after another — a line across the snapshot, a row apart each, a cache miss each — and spun, worked out a sine and a cosine at every one of them, which in a browser are software routines. Now a row is worked out first, and a run of pixels taking the same number of samples gets one sample for the whole run before the next, so neighbouring pixels read neighbouring memory; the steps along the line are worked out once per number of samples. Each pixel adds its samples in the same order, so it is the same blur to the bit, held so against the pixel-at-a-time loop under turned and uneven views, at the sample cap and at the edges. In Chromium over a 12-megapixel photograph fitted to 1170×878: zoom 0.3 7.3 s → 4.0 s a frame, spin 20° 11.9 s → 4.7 s. What is left is mostly the margin a window is drawn with, as wide as the blur reaches, which the filter blurs too.
- ✅ **A filter worked out for the window alone.** A frame is drawn with a margin round the window as wide as the filters reach, so what is inside the window reads its neighbours past the window's edge — and every filter was worked out over the whole margin too, two and a half times the window under a radial blur. Now a surface says what part of it is wanted (`Surface::wanted`, the window on the engine's scratch), and a filter that works from a snapshot — the radial and motion blurs — reads all of the clip and writes only the wanted part grown by `reach_above`: the summed reach of every layer drawn after it, which is what reads its output. The margin is composited but not filtered. Held to the whole page to the bit under a zoom blur, a motion blur over it and a spin blur over that, with the margin's corner checked to be unfiltered, so the trimming is known to be live; and the engine's frame is held to a plain render of the padded surface, whole and repainted a region at a time. Chromium, 12 MP fitted to 1170×878: zoom 0.3 4.0 s → 1.8 s a frame, spin 20° 4.7 s → 2.3 s (7.3 s and 11.9 s two chunks ago). A filter a copy draws is drawn where the copy stands too, under whatever is above the copy, so it is never trimmed (a test shows the window's edge coming out wrong when it was). A group is isolated on a surface of its own, and a layer wearing effects is drawn on one for its silhouette: each is handed what is wanted of the page in its own coordinates, grown by what reads it past that (the effects' reach, the room for softening), so a filter inside a group is worked out for the window too — held the same way with the three blurs inside a plain group and inside one wearing a shadow, where the comparison is within a float's width, since a shadow grown from a silhouette drawn on a surface cut to the region drifts that much whether or not anything is trimmed.
- ✅ **Invert, posterize, photo filter and threshold without a power a pixel.** Each worked on the values a device shows, so each channel of each pixel crossed the sRGB curve there and back — in a browser, where a power is a routine in software, an invert was 43 ms a frame over a 12-megapixel photograph fitted to the window where a curves layer was 9, and a photo filter 65. Now a pass prepares what a curves layer prepares: one table a channel over the light in it (`Prepared::Through`, `CURVE_STEPS` entries, read between them), with what a channel above white is carried on by; and a threshold's level is brought into light once (`Prepared::Level`), the curve being monotone, with the curve itself asked only within a whisker of the level, where the two round apart — a test holds that bit for bit at the level and a float's width either side, holds the smooth ones within five hundred-thousandths of the formula (the most at the encoding's seam, a thousandth of an eight-bit step), and holds a posterize between the two steps either side where a step's edge falls inside a table cell, which is a four-thousandth of the light wide, and on the formula's step everywhere else. The formulas stay, as what the tables are made from and what answers without a pass. Chromium, the same photograph, two runs (the bench moves by half between runs): invert 43 → 17–20 ms, posterize 41 → 19–27, threshold 19 → 7–15, photo filter 65 → 22–23.
- ✅ **A pan is the frame before it, moved over.** A view moved by whole device pixels at the same scale over the same window shows what the last frame showed, shifted: the engine's cache is shifted in place (`shift_rows`), only the strip down one side and the strip along the top or bottom that the move exposed are left to recompute (`Session::pan_to`, a second list of stale regions so the two strips are not one box over most of the window), and the presented bytes are shifted the same way with only those strips encoded again (`last_shift`, `last_repainted`). Every pixel kept is the pixel a fresh frame puts there, since moved a whole number of its own pixels a sample lands where it landed; the strips are a region repaint, within a float's rounding of the whole as every region repaint is. Held to a fresh session at the same view under a blur's reach and a shadow's, beside the page and on it, after an edit, and back to a whole repaint for a move that is not by whole pixels or further than the window is wide. The app rounds the engine's origin to whole device pixels while the view is only panning (`engineOrigin` in App.tsx: a zoom at the pointer lands anywhere, and from then on no pan would have been by whole pixels; at rest and on a zoom the origin is given exactly, so nothing moves that did not — a probe two pixels inside a stroke band found the band half a pixel over when every origin was rounded) and no longer tells the engine its frame is stale after a pan. The presenting is the engine's own now (`Session::present_into`, which the WASM `render_frame` is a thin wrapper over), so it is tested natively: the bytes presented through a run of pans are a fresh session's to the byte — including pans whose exposed strips lie wholly beside the page, where there is nothing to repaint but the frame has still moved, which the cache got right and the presenter missed until smoke 11m, panning a zoomed page about and back, found the page eighteen pixels from where it had been. Profiled in Chromium over the 12-megapixel photograph, forty drag steps spent 15 ms drawing (the strips) and 80 ms shifting, where each step had been a 40 ms redraw; what is left of a step is the page's own work.
- ✅ **A rounded rectangle a row at a time.** A square rectangle, an ellipse and a path each had a scanline fill; a rounded rectangle standing square on the page went pixel by pixel — the exact product away from its corners, and at every corner pixel a box of two hundred and fifty-six point tests, which was most of what a page of two hundred rounded rectangles cost to draw. `fill_rounded_rect_scanlines` works a row through the straight part as the square rectangle's row (the same product, to the bit), gives the columns between the corners that product on every row, and at a corner makes each of thirty-two sub-rows one span from arc to arc, a square root apart, exact across. Held to the pixel-at-a-time fill: to the bit wherever a pixel is clear of the corners, within a few hundredths at them, and the area the same to a fraction of a pixel — small and pill and over-asked radii, fractional places and zooms, flipped, and under a clip. Profiled in Chromium over a page of two hundred rounded rectangles, forty wheel-zoom steps spent 120 ms in the fill where the pixel-at-a-time path had spent 970 ms in point tests, coverage and painting: 3 ms a step where it was 24, with the frame's encoding (9 ms) now the larger part of a zoom. One SVG round-trip test moved: it held a card with a shadow inside a group with an outline to three hundredths of the page after the trip, and the outline, grown from where the group's silhouette is half covered, stands a pixel over wherever the shadow's blur lands within a hair of half — which the corners' five-thousandths now tipped; it judges the geometry with the effects off and allows a feather pixel or so with them on.
- ✅ **A rectangle's stroke band a row at a time.** The band of a plain stroke round a rectangle standing square on the page — rounded or not, inside, centred or outside the edge — went pixel by pixel too: the difference of two products where a pixel is clear of the corners, and at every corner pixel the five-point probe and a box of two hundred and fifty-six point tests of the band, which over a page of two hundred bordered cards was 30 ms of a zoom step. `fill_rect_band_scanlines` is the fill's twin: the outline grown by the band's far side less the outline grown by its near one, each a rounded rectangle of its own (`rounded_span_at`), the clear rows and columns the same difference of products to the bit, and at a corner one span on each side of the band per sub-row, between the two arcs. Held to the pixel-at-a-time band the same way, thin and thicker than the corner's radius and thicker than half the rectangle, round a square corner as well. Profiled in Chromium over the two hundred cards given a three-pixel border, forty wheel-zoom steps spent 132 ms in the band where they had spent 1190 ms in point tests, coverage and painting.
- ✅ **Text painted over its ink alone.** A text block was painted over the whole of its box, every pixel carried into the block, sampled between four texels of the raster and found empty — and most of a block is empty: the leading between lines, the short line's end. A raster now knows where each row's ink is (`TextRaster::ink`, `row_ink`), and a block standing square on the page paints a window row over the ink of the two raster rows it reads, a pixel to spare each way, and nothing else; a turned block takes the whole box as before. Not a pixel comes out differently, since what is skipped would have sampled nothing but noughts — held to the bit against painting the whole box, at the page's size and zoomed, shifted by fractions, flipped, cut by a clip, and turned. Profiled in Chromium over a page of forty text blocks, forty wheel-zoom steps spent 213 ms sampling where they had spent 485 ms: 5 ms a step where it was 12, and the frame's encoding (11 ms) is now the larger part of a text zoom.
- ✅ **A median's channels binned by the encoder's table.** The median filter bins each channel of each pixel by what it shows at — the display encoding's 256 steps — and found the bin by a binary search of the step edges, eight compares a channel, three channels a pixel, before any of Huang's sliding window had begun. It uses the frame encoder's own table now (`Srgb8::channel`, a compare or two), which names the same bin everywhere across the range, below nothing and above white; only within a float of a step edge itself — where the edge, worked out as the light at the midpoint, sits a rounding off the encoder's exact threshold — can the two name either bin, and no pixel of a picture lands there (the test counts those floats and finds a few hundred of two hundred thousand, all at the edges). Chromium, the 12-megapixel photograph at radius 2, the two builds alternated: 130 and 129 ms a frame before, 126 and 101 after — the search was a tenth or so of the filter, the sliding window the rest. The browser benches now invalidate the frame before timing it: a view moved by whole pixels at the same scale is a pan since the pan path, and was being timed as one.
- ✅ **A pan never drafts.** Measured with Chromium's processor throttled four times over, as a phone's is, every pan step of the 12-megapixel photograph was a draft: the last full frame had taken longer than the draft threshold, so each step drew the whole frame again at half size — 150–175 ms a step — where the pan path would have shifted the frame before it and drawn two strips. The view effect now never drafts a pan (the same `panned` that keeps it from invalidating), and a slow page's pans are shifts again: 115 ms a step on the dev server under the same throttling, most of that React in development mode; the production build under the same throttling pans at 89–93 ms a step with no draft and zooms at 150 ms a step drafting every one (unthrottled, a pan step is 50 ms, most of it the bench's own two frames of waiting). Smoke 11m makes the page slow and finds a zoom drafting and a pan not.
- ✅ **The benches, kept.** The two harnesses the numbers above come from were in a scratch directory and are `tools/bench/` now (its README says how): `engine/run.mjs` times every adjustment and filter over a 12-megapixel photograph through the WASM engine alone, with `--prof` for where the time went and a package directory for an A/B against another build; `app/gesture.mjs` drives the editor itself through a drag or a run of wheel zooms, against the dev server or the production build (`app/serve-dist.mjs`), with `THROTTLE=4` for a phone's speed and `PROF=1`. CLAUDE.md points at them, and at keeping `npm run dev` up for looking at app changes rather than rebuilding each time.
- ✅ **An audit of pages nobody wrote through a window, and what it found.** Six hundred of the shared fixture's random pages, each drawn through a window two-thirds its size, panned by whole pixels three times and edited once, held to a session that only ever looked there (the depth method from "Next up" item 3, turned on the day's pan path and window-only filter trimming). It found a crash and two kinds of wrong pixel, all fixed, and is clean now — it ran as a temporary test and left two of its pages behind as regression tests (`pages_nobody_wrote_panned_through_a_window_are_the_fresh_view`, 488 and 14; and `a_window_beside_a_group_of_blurs_on_a_page_nobody_wrote_draws`, 33). The crash: a group wanting nothing of the window (the window lying wholly beside it) stored an empty rectangle at the origin, which grown by the reach above and cut to a blur's clip became an inverted one, and the smears' snapshot-copying helper subtracted past nought; an empty write is nothing to write now. The first wrong pixel: a pan's exposed strips were marked in document pixels and so clamped to the page, on the belief that beside the page there is nothing to draw — but a copy standing past the page's edge is drawn there (page 488), so a strip beside the page was cleared and never drawn again; the strips are kept in the cache's own pixels now (`stale_more`), whole. The second: a text block under a feathered mask near the window's edge came out a little different depending on where the window was, since the engine's margin covered filters and effects but not a feather's softening, which clamped at the window's edge — a seam once a pan keeps the frame before it; the margin covers the page's widest softening now (`softening_reach`). The same audit run against the GPU over three thousand more pages (seeds 2000–5000) found five it draws differently from the reference, all five identical before today's engine changes: two are minimised — an artboard wearing a mask and effects and holding a rectangle larger than itself (4204), and a copy of an artboard that holds a clone layer, wearing a mask and effects, which `copies_a_clone` does not recognise since it follows copies to a clone directly and not into what a copied frame holds (3635) — and are the next GPU work.
- ✅ **A copy of a frame holding a clone layer, dressed, is handed back by the GPU.** `copies_a_clone` follows a copy to its original and asks whether that is a clone layer; a copy of a frame or a group *holding* one was not recognised, and the GPU backend drew such a copy wearing a mask and effects on a surface of its own, where the clone has nothing under it to paint with — page 3635 of the random pages, found by the audit above. `copies_a_clone_within` looks into what a copy copies (through copies of copies), and the backend hands back a dressed copy of anything holding a clone, as it already did a dressed copy of a clone; a plain copy of such a frame is still drawn, and drawn the way the reference draws it. The one GPU difference left from the audit is page 4204, an artboard wearing a mask and effects and holding a rectangle larger than itself.
- ✅ **A coverage under a coverage, on the GPU.** The GPU backend reads one coverage texture a fragment — a layer's mask, what it is held to, the frame it is cut by and a turned frame's cover all ride that one slot, folded together when the coverage is built — and a layer wearing an effect puts its own mask on its own drawing (so the silhouette the effect is grown from is the masked one) by writing it over every slot the drawing left. Those slots were not all empty, and what they held was lost: a masked frame with a shadow stopped cutting what it held (page 4204 of the random pages, the one the audit left open), a masked frame in a turned copy came out upright since its cover went too (3186), and a masked group with an outline lost the cut of the turned frame inside it (2924); a clipped group is the same slot without any effect, so a paint layer inside one, on a surface of its own, was no longer held to the layer below (830 — the fix found this one). Now every coverage built under such a layer folds the layer's own in (`Scene::worn`, the layers above whose coverage rides their drawing, with what each is held back by; set aside while a layer on a surface of its own is drawn, since what is on that surface meets them once, where the surface comes down, and that quad folds them in), every coverage put on a run of quads is recorded (`Scene::placed`, which subsumes the strokes' `settled`), and a layer that writes its own over everything it drew puts the recorded ones back, newest first. Page 2956 was something else: a frame standing wholly outside the frame it is in draws nothing and left as soon as it knew — with the surface opened for it, since it was faded, never closed, so everything drawn after it went into that surface and was never seen; a layer that turns out to draw nothing takes its surface back now (`nothing_drawn`, for a frame outside its frame, a picture with no pixels, a text block or a brush layer with nowhere to go), and page 1115 found the frame arm's other early return leaving its cut on the list. The random-page audit over seeds 0–6000 on the GPU: 3081 pages drawn (the rest handed back), none apart from the reference past the audit's thresholds, where five had been (and 830 and 1115 before this was finished). Tests: `a_coverage_under_a_masked_layer_wearing_an_effect_is_kept` (a masked frame with a shadow holding more than itself, a masked group with an outline holding a turned frame, a turned copy of a masked frame with a shadow, and the six pages, to the audit's thresholds — with the put-back loop emptied, the first fails by eight points inside a shape), `a_frame_outside_its_frame_leaves_what_follows_it_drawn` (fails with the surface left open: the ellipse after the frame is the ground's colour).
- ✅ **A frame's bytes, encoded by runs.** Profiled at a phone's speed (Chromium throttled four times over, the production build), a wheel zoom over a page of forty text blocks spent more of its time turning the finished frame into display bytes than on anything it drew: `Srgb8::encode` was the largest self time, ahead of sampling the text and rasterising glyphs. Two changes, both to the byte. The channel lookup is straight arithmetic now (`Srgb8::channel`): everything at or past one's bits is white unless it is past infinity's (a NaN), a negative number has the sign bit and lands there too, and what is left is one table read and one comparison against a step table padded with a step nothing reaches, where it had been three range tests and a loop that ran once; inlined into the frame loop. And `Surface::encode_srgb8_region` gives a pixel the bytes of the one before it on the row when its bits are that pixel's, since most of a page is a ground or a fill. Natively over a 1170×878 frame: 8.0 ms to 5.0 for the lookup alone; with the runs, 5.5 ms for a frame of noise with no two neighbours alike and 2.0 for one that is mostly flat. In the browser the photograph alone is unchanged within noise (40.6 and 41.0 ms before, 42.4 and 41.0 after), and the text page's wheel zoom at a phone's speed went from 249–286 ms a step to 228–238, both builds alternated. Tests: `a_frame_encoded_by_runs_is_every_pixel_encoded` (runs and lone pixels, the odd values — faint, half there, gone, past white, below nothing, NaN, nought and negative nought side by side, the same colour at another alpha, each channel moved alone — over a whole frame, part of a row, one column, nothing; and the buffer outside the region untouched: dropping any one channel from the comparison, or never refreshing the remembered pixel, fails it); the colour crate's own encoder tests hold the new lookup to the curve's bytes (dropping the comparison, or answering white for a NaN, fails them).
- ✅ **No library call a pixel in the frame's encode.** The runs above saved two-thirds of the encode natively and a tenth in a browser. Timed through the WebAssembly engine alone on the page of text (no React), the reason was the loop's own bookkeeping: comparing the remembered pixel as an array and writing four bytes with `copy_from_slice` are each a call into `memcmp` and `memcpy` in WebAssembly, once a pixel, where natively they are a register compare and one store. The channels are compared one by one now and the bytes written as one four-byte array. A full 1280×760 frame of the text page: 33.6 ms before the runs, 32.2 with them, 25.8 with this; a draft (640×380) 11.7, 11.6, 9.4. The photograph alone, which has few runs but paid the copy at every pixel: 41 ms to 36. A 128-bit key in place of the four comparisons was tried and is a little slower. The text page's phone-speed wheel zoom reads 209–238 ms a step against 231–286 for the build before both changes, though that bench moves by a fifth between runs and the frame times are the numbers to go by.
- ✅ **Text read a row of samples at a time.** A text block's raster was sampled a pixel at a time, each sample working out again which two raster rows it falls between and checking four texels against the raster's edges — and for a block with no shear or turn, every pixel along a row of the window reads the same two rows at the same fraction between them, since the inverse's shear is a signed nought and moves no sum. `TextRaster::rows_at` picks the two rows once for the window's row and `RasterRows::sample_at` reads across them, the same arithmetic in the same order, so a sample is the sample to the bit. Through the WebAssembly engine alone on the page of forty text blocks: a full 1280×760 frame 25.5–26.3 ms to 23.6–23.7, a draft (640×380) 9.7–9.8 to 9.0–9.1, the two builds alternated. Tests: `a_row_of_samples_is_the_samples` (every place tried — between texels and on them, off each edge by half a texel and further, below nothing, NaN — read both ways to the bit; then a two-coloured block drawn both ways to the bit at its own size, zoomed in and out, shifted by fractions, flipped across and upside down, and under a clip: swapping the two rows, dropping the fraction between them or picking the row without the raster's origin each fails it).
- ✅ **Glyph outlines kept.** Every glyph of every text block was read out of the font file again each time a block was rasterised — which is every frame of a zoom, since a block is rasterised at the size it is seen at — though a glyph's outline in font units is the same at every size and place. `kept_outline` keeps them by face and glyph, the way the shaper's answers are kept (`shaped`), and the upright, leaned and turned paths all build their glyph from a kept one; it is let go of wholesale past 8192 glyphs. A glyph from a kept outline is the glyph read, to the bit. Through the WebAssembly engine alone on the page of forty text blocks, the two builds alternated: a draft frame (640×380) 9.0–9.1 ms to 7.9–8.3, a full one (1280×760) 23.6–23.7 to 22.8–23.7. Test: `a_glyph_from_a_kept_outline_is_the_glyph` (a block upright, in a second face, leaned for an italic the face has no oblique for, and set along a ring, at two sizes and off the grid, drawn from outlines read and then twice from kept ones, to the bit: keying the kept outlines without the face, or without the glyph, fails it).
- ✅ **Shadows cast from a coverage, and stamped by rows and columns.** Profiled at a phone's speed, a wheel zoom over a page of sixty shapes with drop shadows took about 760 ms a step, nearly all of it in three places: the blur's two passes and `stamp`, which lays an effect's field down. A shadow's field was the layer's silhouette tinted in all four channels and blurred in all four, then read back in four — when what is in it is one coverage under one tint. Now `silhouette` makes a plane of coverage, blurred by the same box passes made generic over what they sum (`blur::Lane`, for a picture's pixel and for a single channel; `gaussian_blur_cover`), and the tint is laid where the field lands (`Field::Cover`); an outline's band is a coverage the same way, and a bevel's heights, which only ever read a white field's alpha, are that plane. A plane blurred is to the bit the alpha its picture blurs to; a shadow moves by a rounding, two parts in a million at the most with a spread, since the tint multiplies after the sums rather than before; an outline and a bevel do not move at all. And `stamp` worked out where every pixel reads the field — the taps either side, whether each is on the window, how far between — at every pixel, though across it depends on the column alone and down on the row alone: it works them out once a column and once a row now, the same arithmetic. Through the WebAssembly engine alone on the shadows page, the builds alternated: a full 1280×760 frame 133–145 ms to 77–96 (most runs 81–82), a draft (640×380) 29–32 to 19–21. Tests: `a_plane_blurs_to_the_alpha_of_its_picture` (to the bit, over part of a surface and the whole, either edge rule, softnesses up to past the clip; the plane blur ignoring its edge rule fails it), `an_effect_from_a_coverage_is_the_effect_from_its_tinted_picture` (against the tinted picture blurred and stamped as before: shadows at offsets between pixels, with a spread, an inner shadow kept inside, and exactly with no blur and no offset; dropping the tint, reading the coverage without the window's origin, or ignoring the inversion fails it), `a_stamp_by_rows_and_columns_is_the_stamp_by_pixels` (against the per-pixel stamp kept in the tests, to the bit, for a coverage and a picture at whole and fractional offsets either way, a window smaller than what is written, a gain, kept inside and held; swapping the taps, taking the across fraction for down, or reading a tap just off the window fails it).
- ✅ **A layer's surface composited without what is not there.** A layer drawn on a surface of its own — one wearing an effect, or blended — is brought down onto the page by `composite_from`, which sent every pixel of the surface's window through `blend_pixel`: the whole table of modes, called out of line, for each one, including the empty ones round the shape, where every mode's compositing is the backdrop's own value plus nothing. It passes over a pixel with no colour and no cover now (one with colour and no cover is light added, and is composited), reads its rows as slices, and takes Normal — most layers — without the call (`blend_or_over`, which `stamp` uses for the same reason). The same answer to the bit. Through the WebAssembly engine on the page of sixty shapes with shadows, the builds alternated: a full 1280×760 frame 84–89 ms to 65–66, a draft 19–20 to 16–17.5; the photograph and the text page unchanged within noise. Test: `a_layer_composited_by_rows_is_the_layer_composited_by_pixels` (every one of the sixteen modes at full and part opacity, over a backdrop opaque, part there and absent, from a window off the surface's corner a third empty with now and then colour and no cover, against `blend_pixel` a pixel at a time, to the bit: passing over every pixel with no cover, passing over every pixel under half, or taking over for a mode that is not Normal each fails it; and clips with no width, empty and inverted — one with its left edge near the end of the last row, which the first version of this read past both surfaces for, as the gate's `a_rectangle_repainted_is_the_page_drawn_whole` found — composite nothing, and without the guard for them it fails).
- ✅ **The blur's pass along the rows, as runs.** The box pass across a line copied the line in through an index worked out per element, though a row is one slice, and asked at every pixel whether it was in the line's middle stretch — where neither end of the window can run off — or near an end. It copies a row as a slice now and walks the three stretches as three loops, the middle one reading the two ends of the window as slices: the same additions in the same order, held to the reference by `a_blur_in_three_runs_is_the_same_blur` and the blur's other tests (subtracting before adding, or reading the trailing end one along, fails them). Through the WebAssembly engine, the builds alternated: a Gaussian blur filter over the 12-megapixel photograph 41–45 ms to 34, and the page of sixty shadows' full frame 62.7–66.2 to 60.1–62.4 (draft about 16 to 15.3); the motion, radial and surface blurs, which do not use this pass, unchanged.
- ✅ **A picture drawn without a call a pixel it does not need.** Profiled, the 12-megapixel photograph fitted to the view spent most of its frame in `draw_raster`'s row-and-column path, and inside it two calls a pixel that answer the same thing every time: `coverage_at` for a picture with no mask (one, everywhere) and `blend_pixel` for a picture in Normal (the whole table of modes, out of line, to reach `over`). The coverage is said once for an unmasked picture, and every per-pixel blend in the renderer goes through `blend_or_over`, which takes Normal where it is called and hands every other mode to `blend_pixel` — the same arithmetic either way. Through the WebAssembly engine, the builds alternated: the photograph alone 36.2–38.2 ms to 30.3–33.9; the text, shadows and rectangle pages unchanged within noise. Test: `a_blend_with_normal_taken_inline_is_the_blend` (four thousand pairs of pixels — opaque, part there, absent, colour with no cover — in all sixteen modes, to the bit against `blend_pixel`: taking `over` for one more mode fails it); the picture's own tests (`an_upright_picture_is_the_same_picture_by_rows_and_columns` and the rest) hold the drawing.
- ✅ **A list in the shared fixture.** The eleventh shape put in by the method in "Next up" item 3: a text block set as a numbered list (`listed`, two short items at seven points), the one kind of type-setting the standing document had never held — lists had been drawn on pages nobody wrote and in the exporters' own tests, and a paragraph here had been a paragraph throughout. Its edit in the command list takes the numbers off, which moves every line of the block left. Every audit held — the file, the clipboard, the undo runs, the dirty region, the wire, the cross-renderer comparison, resvg and Ghostscript — so the code the list exercises was broken to see who would notice: with a list laid out as its paragraphs (no markers, no indent), exactly one test fails, the list's own (`a_list_hangs_its_items_beside_their_markers`). Nothing else looked, and nothing else *could*: the exporters set type through the same layout, so a reader that is not us agrees with whatever it says, and the GPU reads text off the same raster. So it is asked directly now, of the fixture's own block (`the_fixtures_list_is_numbered_on_the_page`: the block drawn alone against the same block as paragraphs — the words hang past the numbers, and the two differ where the numbers are; the sabotage above fails it). Worth knowing rather than worth fixing, and the shared document is the stronger for holding a list.
- ✅ **A star in the shared fixture, and what it found.** The twelfth shape put in by the method in "Next up" item 3: a live polygon (`star`, five points pulled in by half, corners rounded, ten pixels across in the top right corner), the one kind of shape the standing document had never held — rectangles, ellipses and paths had stood there, and a polygon had been drawn on pages nobody wrote and in its own tests only. Its edit gives it two more points and pulls them in further. Two audits stopped holding. The file format's inventory of fields a `.chitra` cannot be missing grew by three — a polygon's size and its count, as required as a rectangle's width; its depth and its corner radius are additive and are not among them. And the GPU's per-layer audit (`every_layer_of_the_fixture_puts_down_what_the_cpu_puts_down`) said the star was not the layer the reference puts down: eight of its forty-eight pixels differed by more than a tenth. Minimised, that was not the star but a *path* ten pixels across: a pentagon the same size reads the same, an ellipse and a rounded rectangle read nought, since those have analytic coverage on both sides while a path is filled through a stencil and antialiased by four samples a pixel there — the documented approximation — so its edge pixels come out up to a quarter and a bit from the reference's exact area, and a shape that small is more than a tenth edge. The audit's allowance was a tenth of the layer's own pixels, whatever they were; it tells an edge from an inside now (a pixel the reference put down less than nine tenths of the layer's full change at), allows an edge a sample's worth (0.35) and holds everything else to the tenth as before — dimming every fill by a seventh still fails it. Then, everything holding, the shape was broken to see who would notice: a polygon drawn as its outer corners alone, or with sharp corners, fails nothing but its own tests, since every renderer and exporter reaches a polygon through the one path `as_path` makes of it. So the fixture's star is asked directly (`the_fixtures_star_is_a_rounded_star`: drawn alone against the pentagon of its outer corners and against the sharp star of the same numbers, differing from both; either sabotage fails it).
- ✅ **A conic gradient in the shared fixture.** The thirteenth shape put in by the method in "Next up" item 3: an ellipse filled round its centre (`wheel`, in a box that is not square, started a third of a turn round so the seam is on no axis, with a stop that is half there), the one kind of gradient the standing document had never held — a linear and a radial one stood there, and a conic one had been drawn on pages nobody wrote and in the exporters' own tests, where it is the gradient SVG cannot say (it goes as a pattern of a picture of itself) and PDF has no shading for (its layer goes as pixels). Its edit turns the seam half way round. One audit stopped holding: the file format's inventory grew by a conic gradient's centre, its stops and what a stop cannot be missing — eight fields, the same the radial one requires; the angle is additive and is not among them. Then the code it exercises was broken — a turn measured from nought whatever the angle says — to see who would notice: the gradient's own test, and both GPU audits, since the graphics card reads the turn through a shader of its own and no longer agreed; the exporters did not and could not, since what they write of a conic gradient is a picture of what the engine draws. A second witness is worth having, and the fixture's wheel is asked directly as well (`the_fixtures_wheel_turns_round_its_centre`: drawn alone, the page shows through where the half stop is, and it is not the wheel turned half round nor run backwards; the sabotage fails it).
- ✅ **A twirl in the shared fixture, and what it found.** The fourteenth shape put in by the method in "Next up" item 3: a filter that *moves* pixels (`swirled`, a twirl over the middle of the page, a quarter turn easing out to a circle half the page's shorter side across, masked to a disc inside that circle). Two kinds of filter stood here — a blur, a neighbourhood, and grain, a function of where a pixel is — and a warp that reads what is under it from somewhere else on the page never had; its edit turns it the other way. One audit stopped holding: the file format's inventory grew by how far a twirl turns and by an ellipse mask's size — the fixture's masks had been rectangles, paths, strokes and a picture. Then the code was broken to see who would notice. The twirl's direction reversed: its own tests and both GPU audits, the graphics card reading the turn through a shader of its own. Its *reach* cut to a tenth — the margin the engine pads every repaint by, so a region drawn on its own reads the pixel a turn away rather than its own edge repeated — nothing: the padded-region audit over the fixture asks a dozen rectangles and none crosses the disc, and the page's widest reach is the blur's anyway, so a twirl saying less is never what the engine pads by on this page. So the fixture's twirl is asked directly (`the_fixtures_twirl_turns_inside_its_disc_and_nothing_outside`: the page with the twirl against the page with it hidden differs inside the disc and nowhere outside, to the bit; turned the other way it is another whirl; and three regions across the disc, each drawn on its own padded by the twirl's own reach, are the whole page there — the reach cut to a tenth fails it).
- ✅ **A mask inside a masked layer, drawn by the GPU.** The backend handed back any masked layer holding something masked, and any masked layer whose strokes carried regions: one coverage slot, and a layer wearing an effect wrote its own mask over everything it drew, so a mask inside was dropped — measured, when the decline was written, as a masked group holding a masked ellipse and casting a shadow coming out as the reference's page with the ellipse's mask *removed*. Since the coverage records of two days ago, every coverage built under such a layer folds the layer's own mask in and is put back once the layer's has been written, so the pair draws right; the decline is lifted and the random pages go from 3081 drawn to 3107 with none apart from the reference (seeds 0–6000). Test: `a_mask_inside_a_masked_layer_is_drawn` (the pair from the decline's own comment, and a masked frame holding a masked rectangle wider than itself under an outline, each against the reference to the audit's thresholds and each told from the same page with the inner mask taken off: building coverages without the fold, or writing the layer's mask without putting the records back, fails it).
- **What the random pages really go back for, and the biggest reason lifted.** Rather than reading the GPU backend's declines off its comments, every `return None` on its gathering road became a `none!()` that notes its line, and a standing ignored test (`what_the_random_pages_go_back_for`, run by name with `--ignored --nocapture`) tallies six thousand random pages by the line that refused them. The first reading: 2893 of 6000 refused, and 719 of those — a quarter — by one rule, a layer held to an adjustment, a filter or a clone layer, on the grounds that such a base drawn aside is nothing. The reference confines nothing to an adjustment or a filter (a layer held to one shows whole), and it says so in one place now, `confines_nothing` in `core/render`, which `draw_layer` reads and which `clip_coverage_at` — the cut the GPU already reads for every held layer — answers through. So the GPU simply stops refusing those two; the cut it reads is the page's own answer, everything let through, and being held still means the rest of what it means (a frame or a copy on a surface of its own). The two wrong lifts were tried first and are the test's teeth: "no base at all" drew a frame held to an adjustment in place, where a Color Burn inside it met the page rather than nothing (seed 5601), so a held frame still goes back; and a clone base stays refused, since what is held to one is confined to the strokes it laid and drawn aside it lays nothing (a halftone held to a masked clone, seed 1176, was the whole page's halftone against none of it). `a_layer_held_to_an_adjustment_shows_whole` asks all three directly, and the random pages went from 3107 drawn to 3416 with none apart. The next three by the tally: a blended layer wearing a bevel (453), a picture with its corners moved (458), and a copy with stand-ins reading the backdrop (256).
- **A blended layer's bevel, drawn by the GPU.** The second decline by the tally above: a bevel is lit at the stamp, and a blend mode takes the stamp over to bring a picture down by, so a blended layer wearing a bevel went back (453 of the random pages). A blended layer's other effects already settle first — the offset and the hold-inside taken on a scratch pass, `fs_settle`, with the field and the layer's own surface both bound, leaving the stamp an ordinary picture — and a bevel's half is lit from exactly those two: the heights on the field and the layer's coverage. So the settling pass lights it (`bevel_lit`, the same function the stamp calls), carrying the light, the half's colour and the heights the way the stamp carried them, and the stamp brings the lit half down by the blend like anything else. `a_bevel_lights_what_the_cpu_lights` asks Multiply, Screen, Overlay and Difference against the reference and against the same page without the bevel, so a settling pass that lit nothing cannot pass. The random pages went from 3416 drawn to 3705, and one of the new ones is apart — seed 2333, a single interior point at 0.244 — for a reason that is not the bevel's: the same page with the blend set to Normal, or with the bevel taken off, was always drawn and is apart by exactly the same amount. Its path has a sliver a fifth of a pixel wide at one row, and an outline band is measured from a yes or a no at half coverage: the reference's exact area says no there and four samples say yes, so the band spreads its width from a pixel the reference never counted. That is the stencil-and-four-samples approximation the backend is documented to make, met at its sharpest; it is the only one of the 3705.
- **A picture with its corners moved, drawn by the GPU.** The biggest decline left by the tally once the two above were lifted: a picture put into perspective (479 of the random pages) is drawn through a projective map, which a quad's two triangles cannot carry — they interpolate straight lines, and the picture is not straight across either of them. So the fragment carries the map *back* instead (`fs_warped`: eight of its nine numbers on the quad's two spare vectors, scaled so the ninth is one at the quad's middle and the denominator positive over all of it) and reads the picture through it a pixel at a time, the way `draw_raster_warped` does: the exact area of each pixel the quad covers, worked out by clipping the pixel's square to the four lines the map sends the picture's own edges to; the footprint read off the map's slope at that pixel; and a shrunk pixel read from two levels of the picture's pyramid and mixed by how far between them it is. The level a pixel wants changes across the picture — the far end of a road is shrunk more than the near — so every level stands under the picture in one texture (`warped_atlas`, each row padded to the picture's width) and the fragment finds a level's rows by halving down to it. `a_picture_with_its_corners_moved_is_drawn_as_the_cpu_draws_it` asks five placements — in perspective at about its own size, a road shrinking to a sixth at its far end, enlarged three times, turned, and faded under a mask — to the audit's thresholds and against the same picture with its corners put back; mixing no levels and taking every edge pixel whole each fail it. The random pages went from 3705 drawn to 4065, with seed 2333 still the one apart, for its own reason above. The tally now: a copy with stand-ins reading the backdrop (325), a layer held to a clone layer (271), a copy of a clone wearing anything (191), and a run of frame cases between 147 and 180 each.
- **A copy standing in where the group reads the page, drawn by the GPU.** The next decline by the tally (325): a copy of a group with stand-ins of its own draws a list of layers rather than the group, and when one of them reads the page — an adjustment, a filter, a clone, a blend — the reference puts the copy on a surface of its own (`isolate` in its copy arm) so what is inside it changes only what is inside it; this backend drew the list straight in, a different picture, and so handed the page back. It asks the reference's own question now (`copy_isolates`, `any_reads_backdrop` over the copy's parts) and puts such a copy on a surface, where its fade, mask and the layer it is held to are taken once on the way down, as every isolated layer's are. `a_copy_standing_in_where_the_group_reads_the_backdrop_is_isolated` asks it against the reference and asks the ground beside the copy's mark to be untouched by the exposure inside the copy; drawing it straight in fails both. The random pages went from 4065 drawn to 4387, with seed 2333 still the one apart. What is left by the tally, 1613 pages: a layer held to a clone layer (272) and a copy of a clone wearing anything (191), both of which want what the clone laid, which only the page has in hand; and the frames — one faded or masked while holding what reads the page (180), one held to the layer under it (153), a blended one holding what works on the page (147), one wearing an effect while holding what reads the page (168), and a turned one held or wearing inside a bound (160) — most of which the reference draws in place and mixes back, a surface that starts as a copy of the page and comes down by a per-pixel mix, which this backend has no pass for yet.
- **A faded or masked frame holding what reads the page, drawn by the GPU.** The first of the frame declines by the tally (180): an upright frame, faded or masked and brought down by Normal, holding an adjustment, a filter, a clone or a blend. On a surface of its own what it holds would have nothing under it, so the reference draws such a frame where it stands and mixes it back by its fade and its mask — `lerp(under, drawn, a)` — and this backend, with no pass for that, handed the page back. It has the pass now, in two halves that fit the machinery already there. The frame goes on a surface of its own that *begins as a copy of what is under it*: the first thing drawn after the surface is opened is a quad over the whole of it reading the copy of the surface below (`Draw::Under`; the executor takes that copy before the pass, the way it takes one for a blend, and the plan says which surface it is a copy of), put in before the frame's drawing is marked so that neither the frame's mask nor its cut is ever put on the copy. Then the frame is drawn into that in place, ground and contents, exactly as a plain frame is drawn on the page. And it comes down *mixed* rather than laid (`Draw::Mix`, `Opening::Mix`): the blend pipeline past its last mode, where the fragment returns `mix(under, drawn, fade × mask)`, which is the reference's arithmetic to the letter — where the frame drew nothing the surface is still the copy and the mix is nothing. A frame that turns out to draw nothing (moved off the page) takes the copy back with the surface it opened, since a surface opened and never closed swallows everything after it. `a_faded_or_masked_frame_is_composited_whole` asks four such frames, an invert inside each — faded, masked, both, and inside a faded group — at two views to the audit's thresholds, asks the invert to have read the page, and asks the off-page frame to leave the page as it was; leaving the copy out and laying the surface over instead of mixing each fail it. The random pages went from 4387 drawn to 4509, with seed 2333 still the one apart.
- **A held frame, and a dressed copy of an adjustment or a blend, drawn by the GPU.** Two more by the tally. A frame held to the layer under it (170): the reference puts a held frame on a surface of its own and cuts the surface to that layer's alpha on the way down, which is what the quad that brings a surface down already does here with what a layer is held to; the frame's arm drew in place and returned before that cut, so a held frame went back. An upright held frame goes on a surface now (`alone`), and `a_frame_between_pixels_is_drawn_and_what_it_cannot_hold_goes_back` moves it to the drawn side, cut away outside the ellipse and its ground showing inside. A copy of an adjustment, a filter or a blended layer wearing a mask or a fade, or a copy of an adjustment held to the layer under it (167): such a copy draws a change to what is under it and cannot go on a surface — there is nothing under a fresh surface to change — and a mask or a fade was exactly what sent a copy to one, so it went back. It is kept off the surface now (`dressed_rewrite`), its mask and what it is held to ride `Scene::worn` into whatever the copied layer builds (the adjustment's own quad; the surface a blended layer comes down on) and its fade is inherited by it, which is the reference's `lerp(before, drawn, cover)` since a mix of a mix by the same base is one mix by the product. Three tests that asserted the refusal assert the drawing instead, each against the reference and against the copy hidden, and with the ground beside a held copy left to the original alone. **A reference defect found on the way, and left for its own chunk:** a copy of a *blended* layer that is only *held* — no mask, no fade — takes the reference's surface road rather than its cover road, so the blend it copies meets nothing and is spent (a held copy of a Multiply text came out in the text's own colour), while this backend draws the blend meeting the page under it, which is the picture the reference's own rule describes. Such a copy stays declined here until the reference is fixed, so that a page is not drawn right against a reference drawn wrong (pages 886 and 5564). So does a dressed copy of a blended layer that *wears effects*: the reference draws it where it stands and mixes the whole composite back by the cover, and a fade reaching the shadow and the layer each on its own is not that where they meet — the in-place-and-mixed pass the frames got is the shape of its answer, another day. The random pages went from 4509 drawn to 4805, with seed 2333 still the one apart.
- **A held copy of a blended layer keeps the blend it copies — a reference defect, fixed.** Found by the GPU backend the chunk before: a copy of a blended layer, held to the layer under it and wearing nothing else, took the reference's surface road (every held layer's), where the blend it copies met a transparent page and was spent — a held copy of a Multiply text came out in the text's own colour — while the backend drew the blend meeting the page, which is the picture the reference's own copy arm describes for a mask or a fade on such a copy (drawn where it stands, mixed back by a cover). Being held takes that road now: in `draw_layer`, a held copy of a blended layer folds the cover it is held by, its own mask and its fade into one cover and hands it to the copied layer drawn where the copy stands, which is what the copy arm does for the other two and what the GPU does with `Scene::worn`. `a_held_copy_of_a_blended_layer_keeps_the_blend` asks a held copy of a Multiply rectangle to multiply inside the shape it is held to and show nothing beyond it, masked or not; with the branch off it comes out in the rectangle's colour. The GPU stops declining such a copy, its copy-of-a-blend test gains a held case with the copied layer held as well, and pages 886 and 5564 agree: the random pages went from 4805 drawn to 4825, with seed 2333 still the one apart. The SVG witness caught the exporter the same way the backend caught the renderer: a held copy of a blended layer travels as the copied layer's markup inside the wrapper that holds it, which isolates it, so a reader spent the blend exactly as the engine used to and the two had agreed; the wrapper takes the copied blend now, as it takes a layer's own (`a_held_or_masked_copy_of_a_blend_keeps_its_blend_in_svg`, and page 744 of the blended pages agrees with its reader again).
- **An effect held to a brush layer, drawn by the GPU — and the soft cut it was squaring.** A small one by the tally (66), and the drift it went back for was real: a layer wearing an effect, held to a brush layer, came out a quarter short along the whole of the brush's edge. Asked why rather than tolerated, the cause was the backend's and not the open question about soft clips: a layer with effects takes what it is held to (and the frame it is in) on its drawing, so that the effects grow from the cut silhouette, and then took it *again* on the quad that lays its surface down — `Item::mask` was one coverage for the effects and the way down alike — so wherever the cut is soft the layer was cut by the coverage squared. The reference cuts a held layer once, before anything is made of it, and cuts each effect by the base as well. So the surface's own way down has a coverage of its own now (`Draw::Close::down`: the masks of the layers above and nothing else), the effects keep the cut, and `an_effect_held_to_a_brush_layer_is_drawn` asks a shadow, an outline and an inner shadow held to a brushed shape against the reference with the bare held layer's own soft edge as the yardstick for the worst pixel — a thousandth, where it had been a quarter; giving the way down the effects' coverage again fails it. A hard cut — an upright base, a frame's whole pixels — squared to itself, which is why nothing had noticed. The random pages went from 4825 drawn to 4883, with seed 2333 still the one apart. What the tally leaves: a layer held to a clone layer (286) and a copy of a clone wearing anything (210), both wanting what the clone laid; the frames — one wearing an effect while holding what reads the page (163), a turned one held or wearing inside a bound (161), a blended one holding what works on the page (152), a turned blended one (68), a base wearing effects (33) — and a pixelate under a turned view (25).
- **A layer held to a frame wearing effects, drawn by the GPU.** The smallest of the frame declines by the tally (33), and the one the double cut above explains: a frame wearing effects as the base of a clip run stayed the CPU's on a sixtieth of drift on a page nobody wrote, measured while a surface's way down was still taking the cut twice. Asked again with the way down cutting once, a layer held to a frame casting a shadow — plain and Multiply — is the reference's picture to the audit's thresholds, interiors included, and so are the random pages that hold one: 4883 drawn to 4911, with seed 2333 still the one apart. `a_layer_held_to_a_base_with_a_shadow_is_held_to_the_base` asks the frame base beside the rectangle it always asked; the decline put back fails it.
- **A turned frame held, or wearing an effect inside another frame, drawn by the GPU.** Next by the tally (161): a turned frame held to the layer under it, or casting an effect from inside another frame, went back for the one coverage slot — the outer bound laid over everything the layer drew would have taken the inner cut with it. Two things since have made that untrue: the coverages put back newest first (`Scene::placed`) keep each quad's own, and a surface's way down takes only the masks above (`Draw::Close::down`). Asked after both, a turned frame held to an ellipse, and one casting a shadow from inside an upright frame smaller than itself, are the reference's picture, cut on both counts: the ground shows inside the turned frame where the ellipse or the outer frame is not, the frame's own ground where they are. `a_turned_frame_is_cut_the_way_the_cpu_cuts_it` asks both beside the four it asked; the decline put back fails it. The random pages went from 4911 drawn to 5047, with seed 2333 still the one apart. What the tally leaves: a layer held to a clone layer (286) and a copy of a clone wearing anything (210), a frame wearing an effect while holding what reads the page (163), a blended frame holding what works on the page (152) and a turned blended one (68), and a pixelate under a turned view (25).
- **A picture with its corners moved, in the shared fixture.** The twelfth shape put in by the method in "Next up" item 3, once the GPU could draw one: a third picture on the same bytes as the other two, in perspective (`cornered`, corners sticking out past its rectangle by three units each way, so that a box reckoned from the rectangle misses pixels), with an edit in the command list that moves the corners. A placed picture had stood upright, scaled and turned here, every one of them a matrix; this one goes to its corners by a projective map that cannot be written as one, so a file, a clipboard, an exporter and the second renderer are each asked to carry four corners. Every audit held, so the code the picture exercises was broken three ways to see who would notice. The reference drawing it as its rectangle: twelve tests, the fixture's own cross-renderer audit among them (`every_layer_of_the_fixture_puts_down_what_the_cpu_puts_down`), the GPU's random pages, and the SVG readers on theirs. A file dropping the corners: the container audits over the fixture's commands and a page nobody wrote, the engine's wire and undo audits, and the renderer's own. A picture's box ignoring its corners, at both places the renderer reads them: the renderer's direct tests and one of the engine's (`a_picture_s_corners_move_by_one_command`), but **not** the fixture's dirty-region audit, with the corners out by three units — past the two-pixel margin that audit says it absorbs — and not when the picture was given a plain move in the command list either, the one edit that asks its box and nothing else. The reason is the fixture's, not the audit's: the standing document holds filters that read a neighbourhood, and once a page does, every dirty region is grown by the page's widest reach, which on this page is wider than four pixels. So on the fixture a box short by less than the page's reach cannot be seen, by construction, and the direct test is what pins the box. The move stays in the command list — one more placement of a picture in perspective the wire, the file and the undo stack carry — and the standing document is the stronger for holding one.
- **Three found by shifting the seeds — dashes on a rounded rect, a frame's filter region, and the page's edge under an effect.** The audits run over seeds 0..N; every one run over N..2N instead (a quarter of an hour, by a script in scratch) is a fresh page nobody wrote, and three of what that turned up are fixed here. (1) A dashed rounded rect's dashes sat a quarter arc out of step with every SVG reader's: a dash pattern starts where its ring starts, and SVG's rect starts at the top edge's left end where the first corner's arc comes out, while the engine's rounded ring (`shape_rings`) started where that arc goes in, a quarter arc before — so every dashed rounded rect in an exported file had a dash where the reader had a gap, along the whole top edge and round from there (page 825 of the covers audit, 63 pixels by a whole colour). The ring starts where SVG's and PDF's do now; the square-cornered ring already did, and a dash on an ellipse already did. And a dashed band inside or outside a rect's or an ellipse's edge wrote no caps or joins — a band lying inside a closed outline has no ends to say anything about, until it is dashed and every dash has two — so a reader squared off every round-capped dash. (2) A frame wearing an effect was filtered over its exact box, where the engine cuts an upright frame to whole pixels of the page and the clipPath was already written from that cut (`frame_cut`): a frame ending a fraction past a pixel's edge fills that pixel on the page, and its filter stopped short of it, so a reader left the frame's last column out wherever it wore an effect (page 1049). The filter's region is the cut now. (3) A reference defect, found as page 1083: a layer wearing effects is drawn onto a surface of its own, *cut to the page*, and its effects are grown from the silhouette on that surface, past whose edge there is nothing — right for a layer near the page's edge, where repeating the edge would invent silhouette, and wrong for a layer *across* it: the part past the page was never on the surface, so a shadow pulled back onto the page from a rect running off the right stopped short of the edge (a reader draws it whole), an outline grew a band along the cut, and a bevel's heights fell to nothing at the edge and lit a cliff there. Now a layer whose padded extent runs past the page, wearing an effect that puts something down (`Effect::draws` — one at no opacity is no effect and may not send the layer down another road, which `an_effect_that_draws_nothing_is_no_effect` enforced on seed 101 when it did), is drawn on the page widened by the effects' reach and the page's pixels taken back (`draw_with_room`, once; a cover taken of it comes back cut to the page; a copy of an effected layer, whose extent is Everything, is taken to run past too). A feathered mask and a filter inside the layer still soften against the page's edge as they always have — the margin is for effects alone, so that an effect at no opacity changes nothing. The margin is capped at the page's own size, since a file can say a reach nobody could mean. One engine test moved a third of a pixel: at (9, 5) a sample of the sampler's grid fell exactly on an edge, and the one more translation in the composition flipped it. The GPU backend, validated against the reference, follows: every surface there is the one size, so the whole scene is drawn that much wider when a layer wants the room (`with_room`, the view moved by the margin and the surface asked for cut out of the middle afterwards), and inside a layer's own surface a brush stroke, a frame and a surface's way down are laid over the whole surface rather than over the page alone (`Scene::inside`, `laid`), as the reference draws a layer whole onto a surface of its own; the page itself is still laid over the page. Two things that found: a clone's lift was bounded by the surface rather than by the page in the shaders (`fs_clone`, `fs_clone_aside`, the heal sums), so with room past the page it lifted what layers crossing the edge had drawn there, which the reference never lays — bounded by the page now, as the reference's "off the page there is nothing to clone"; and clone layers and copies of them take no room, since the reference grows their effects from the strokes they lay, cut at the page as ever. The random pages drawn are 5047 of 6000, as before, with none apart. And a viewer for one seed, ignored, `CHITRAKAR_SEED=n cargo test -p chitrakar-codecs show_a_page_nobody_wrote -- --ignored --nocapture`: the layers, the SVG, and a map of where a reader covers the page otherwise. Measured on the way and left for later: the identity render and the same page drawn with a margin of sixteen all round (`render_region_at` with the view moved) differ on 22 of 300 random pages, the worst by 0.87 — the room given here closes only the effects' share of that (two pages); the rest is feathered masks, filters and whatever else reads the page's edge, and is a page looking different at two views.
  - **Tests:** `a_dashed_rounded_rect_dashes_where_a_reader_does` (four alignments against resvg, witnessed to fail at 62 and 49 pixels with either half taken back out); `a_frame_wearing_an_effect_is_filtered_over_its_whole_cut` (resvg, 13 pixels with the region back on the exact box); `the_pages_edge_casts_no_shadow_and_wears_no_outline` (a rect off each of the four edges, a shadow pulled back over it and an outline, held bit for bit to the same page drawn with a margin of twelve all round — 43 pixels by 0.86 before); the GPU's `pages_nobody_wrote_are_drawn_the_way_the_cpu_draws_them` over its 2000, which caught the brush, the surface's way down and the clone's lift in turn (seeds 1718, 770, 1326); the shifted-seed covers audit runs clean over 800..1600 now. Still open from the same sweep, in the render audits over the shifted seeds: a drop shadow at no opacity changing 8 pixels (seed 160), a mask that hides nothing changing one (663), blends changing coverage at 5 (2302); and in the codecs, a reader blending page 831 otherwise, ghostscript 341 and 369, and the import round trip at 502, 548 and 667.
- **Verify before committing:** `cargo test --workspace` (~620),
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all`,
  and in `app/`: `npm run build && npm run test:e2e` (~1400 browser
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
  quiet, and it asserts that it found some to take off. It takes them off
  and then puts each back wherever the page is still accepted with it
  there, so as the backend learns another kind of effect the comparison
  widens by itself rather than waiting to be told.
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
- **Explained, and fixed:** a middle-drag begun *on top of a picked
  layer* appeared to carry the layer as well as the view — exact and
  repeatable on the runner, unreproducible on a developer machine, the
  layer moving by what looked like (pointer + view) over the zoom and
  came back as 88 document pixels every run. It was not the pan carrying
  the layer at all, and no arithmetic about zoom was involved. On X11 the
  middle button pastes the primary selection, and the middle button is
  this app's own way of carrying the view, so every middle-drag arrived
  as a paste event with nothing in it; the in-app clipboard held a layer
  by then, because the block before it (8y) copies one; the layer that
  came in became the *picked* one, and the X read afterwards was that
  layer's rather than a moved layer's. The 88 pixels were the gap between
  the two layers, which is why the figure was exact every run and why it
  could not be reproduced on a machine where the paste did not fire. See
  the paste handler in `App.tsx` — the event serves a picture from
  another application and nothing else now — and 8y2, which asks about
  the gesture directly. 9y makes the stricter claim again, aimed at the
  picked layer, and it can be made to fail on purpose: put the in-app
  fallback back in the paste handler and it reports `200 -> 72`.
- **The subject of a photograph:** `Session::pick_subject`, for the
  region no marquee can be dragged round and the wand asks the wrong
  question about — a coat, a face and a hand are three colours, and
  spreading from a click on any of them stops at the next. What holds a
  subject together is not a colour of its own but that it is *not* the
  background, and the one thing a photograph says about its background
  without being asked is that the background runs off the edges of the
  frame: a portrait's wall meets all four and the face meets none.
  A judgement from two sides, then. A band just inside the frame is read
  for the colours the background comes in; whatever those colours cannot
  account for is read for the colours the subject comes in; and every
  pixel is asked which of the two spreads it looks more like and by how
  much — a number, not a verdict. Spreads rather than averages, since a
  background is usually several (a wall and a floor, a sky and a
  horizon) and their average is a colour appearing nowhere in the
  picture; blurred across the colour bins, so a colour the picture never
  quite showed is still recognised as one of them, which a gradient
  needs. Read on what the screen shows, the same footing the wand judges
  likeness on, and confined to the pixels the layer actually occupies —
  the shared `Session::layer_inside`, which is also what "pick out what
  this layer covers" is built from — so this is a question about a
  photograph rather than about the page it was placed on: pick the
  picture first, or the page's own edges get read and the answer is the
  whole picture. Though which picture is usually not worth asking: the
  picked layer if there is one, since an explicit choice beats a guess,
  and otherwise the one picture in the document.
  Three things make it work on a photograph rather than only on a
  diagram, and each was measured rather than guessed — against a set of
  pictures with known answers, scored as overlap, because "looks better"
  is not a number and the first version looked fine on everything easy.
  **The numbers are smoothed over the picture before any line is drawn
  through them**, which is most of it: judged a pixel at a time, grain
  and texture and a hundred-colour background all cross any fixed line
  back and forth between neighbours, and what comes back is lace. A
  subject is a *region*, so a pixel's neighbours are evidence about it.
  **The spreads are then read again from what was decided, and the
  decision made again, a few times over**, which is what lets a bad
  start be walked out of: a subject cropped by the frame — a portrait cut
  off at the waist, which is most portraits — puts its own colours into
  the band the background was read from, and read once, that picture
  comes back with nothing in it at all. **And the subject's side is
  seeded from what the background cannot explain rather than from a box
  in the middle of the frame.** The box is the obvious thing and is
  wrong: it is only the subject when the subject fills it, and around a
  narrow figure it takes in as much sky as coat, whereupon a spread
  taught half on sky answers "sky" about the sky. The frame's middle
  survives only as the last resort for the cropped case, where the
  background's own colours do explain everything; leaning on it in the
  judgement itself measured worse on every picture tried, a subject off
  to one side being exactly what it gets wrong. The work is done on a
  copy of the picture a few hundred pixels across — partly cost, since
  it is done several times over, but mostly because grain is not
  evidence — while the region is still traced at full size, so the edge
  that comes back is the picture's and not the grid's.
  Two cleanups then earn their keep, and a flat test fixture missed both
  until a noisy one was tried: a gap of background colour with subject
  all round it is a hole rather than a piece of the background — a shirt
  the colour of the wall is still the shirt — so the background is
  flooded in from the frame's edge, the one place it is known to be, and
  whatever the flood never reaches is given back; and a scattering of odd
  pixels is not a subject, so pieces far smaller than the largest go,
  measured against the largest rather than against the picture so that
  two people both survive and a bird alone in a sky is not mistaken for
  noise by being small. The softness rides along in the same command,
  because a matte off a photograph almost always wants one and a pick
  followed by a softening would be two things to undo where the hand did
  one.
  The tolerance is the knob this needs — whether a shadow under a chin
  belongs to the face or to the floor is not a thing any fixed number
  gets right for every photograph — and it is how readily a colour counts
  as background, a half leaving the decision to the picture. A half
  because that is where it scored best, not because a half is a pleasing
  place to start, and it gives ground gently either side rather than
  falling over.
  It stays a judgement about colour and not a model of what a person
  looks like, and there is a wall there worth naming so it is not walked
  into twice. A *neutral* part of a subject against a neutral background
  — a white school shirt in front of a white curtain, a groom in white
  against white flowers — has no evidence of its own either way, and
  measured on real photographs that is where it fails: the coloured half
  of a subject comes back cleanly and the white half is left behind. The
  principled answer to exactly that is the least-cost way of cutting the
  whole picture at once, a minimum cut over the pixels with the cost of
  splitting neighbours falling where the picture changes, so a region
  with no evidence goes with the company it keeps. It was built —
  Dinic's algorithm over the reduced grid, contrast-sensitive pair costs,
  the spreads refitted between cuts — measured, and taken back out: it
  scored slightly *worse* than the smoothing above on every picture with
  a known answer, cost three times the time and a few hundred lines, and
  did not fix the case it was built for. A white shirt is a large region
  whose own evidence says background, and a prior on its *edge* cannot
  outvote its *area* however strongly it is weighted — at forty times
  the weight that held nothing and only started eating the background
  elsewhere. Getting that right needs a model of what a person looks
  like, which is a different kind of thing from anything else in here and
  has no licence-clean weights to ship. So the honest place for the last
  bit is the region itself: it comes back as a region like any other, and
  a shirt the wand adds in one shift-click is a better answer than a
  promise the method cannot keep.
- **The subject, asked of the system:** `Session::pick_matte` and the
  shell's `subject_matte` command. The colour method above has a wall in
  front of it that no amount of tuning gets past — a *neutral* part of a
  subject against a neutral background has nothing locally to say which
  it is, and a white school shirt in front of a white curtain came back
  missing every time. Apple platforms ship the model Photos uses to lift
  a subject off its background (`VNGenerateForegroundInstanceMaskRequest`),
  which has seen enough people to know the shirt is a shirt; on the
  photographs that defeated the colour method it is essentially exact.
  Nothing is shipped for it — no weights in the repository, no licence to
  read, no download on first use — because it is already on the machine,
  which is the whole reason for preferring it to carrying a model of our
  own. `shells/tauri/swift/subject.swift` does the asking behind two C
  functions, `build.rs` compiles it only on macOS and only when there is
  a `swiftc` to do it with, and a build without one still builds: the
  command answers "nothing here to ask" and the app falls back to the
  engine's own pick, which is what the browser and every non-Apple
  platform get. So the feature degrades by platform rather than failing
  by platform.
  The matte comes back as a matte rather than as an outline, which is the
  point: `MaskKind::Raster` already existed for exactly this shape of
  thing, so a model's soft edge on hair stays soft where a traced region
  would have hardened it, and it is content-addressed, saved with the
  document and undone in one press like any other selection. Two things
  had to give way for it. `Session::region_rings` only knew how to answer
  for a *shape*, so any other kind of region had no outline, no box and
  nothing to add to — which had quietly been true of picking out a
  layer's brushed mask all along; it now draws the coverage and traces it
  at half, so the ants work for every kind of region. And the picture
  crosses to the shell as a shrunk PNG and the matte crosses back as a
  PNG rather than as bytes: a silhouette is a hundred kilobytes encoded
  against a couple of megabytes raw, and that is the difference between
  instant and a visible pause.
  The same entry point is fed by a U^2-Net ONNX model where there is no
  system one — `shells/.../src/onnx.rs`, through `tract`, which is pure
  Rust so there is no C++ toolchain to install and nothing to link. It is
  the second thing tried rather than the first, and on Apple platforms it
  also catches the case where the system model declines to find
  anything.
  The model is *found*, never carried, and that was a measurement rather
  than a preference. The full one is 176 MB, which is not a thing to put
  in a repository; the small one is 4.6 MB and would fit, and was tried
  against the same photographs: it makes a soft, roughly-right shape of a
  child against a plain curtain and finds *nothing at all* in a wedding
  photograph of two people against a wall of flowers. Shipping that would
  be shipping the appearance of the feature. So `~/.cache/chitrakar/u2net.onnx`
  or `CHITRAKAR_SUBJECT_MODEL` if somebody has put one there, and the
  colour pick if not. Which leaves one thing undone and worth doing
  deliberately rather than by default: offering to fetch it, with the
  size said out loud, instead of a silent 176 MB on first press. The
  browser is still on the colour pick — running this in WASM means either
  tract compiled into the engine or onnxruntime-web beside it, and that
  is a bundle-size decision of its own.
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
- **A shadow is the same near the page's edge as away from it.** Put a
  shadow on the shared fixture's *group* — the one thing the fixture had
  never held, since every effect in it hung on a layer — and two audits
  stopped holding at once. The one that mattered: a group copied and
  pasted twelve pixels across drew a different shadow from the one it was
  copied from. Every effect is built over a window (the layer's box grown
  by how far the effect reaches) and the field is nothing at that
  window's edge — but the window is also cut by the *surface*, and there
  the field's edge is not nothing. Both the box passes and the stamp were
  repeating that edge, which invents silhouette that was never there: a
  layer near the page's edge cast a heavier shadow than the same layer in
  the middle. Both read nothing past the window now
  (`blur::Beyond`, and the stamp's own four taps), on both renderers.
  Held against the same layer twenty pixels in, which is the ground
  truth: past the effect's reach from the edge the two agree exactly, and
  nowhere is the near one heavier
  (`a_shadow_is_the_same_near_the_pages_edge_as_away_from_it`). What is
  left is that the near one can still be *short*, where the field it
  would need lies off the surface entirely and there is nowhere to put
  it; closing that means a layer window that reaches past the page, which
  is a change to every device coordinate in the renderer and not worth it
  for a hundredth of a level in two pixels.
  The other audit was the method's other half: wrapping a layer in a
  group compared pages byte for byte, and a blur summed over a window
  that moved does not land on the same last bit. That one now asks for
  the same *picture* rather than the same bytes, which is what it always
  meant.
- **A clone stroke stays inside its region too.** A region picked out
  confines what is painted, and it rides on the stroke so that it goes on
  confining it after the region is let go of. The brush read that; the
  clone did not — so painting a patch out with a region picked spilled
  past it, and spilled with *what the page holds somewhere else*, which
  is the one kind of paint nobody can see coming. What a stroke is filled
  with is no part of the question: the region confines the stroke
  (`a_clone_stroke_laid_in_a_region_stays_in_it`). Found while teaching
  the GPU backend to draw a clone layer — reading the CPU renderer
  closely enough to copy it is a way of reading it closely enough to
  catch it.
- **One idea of distance.** An outline's band is how far a pixel is from
  the layer's silhouette, and it was worked out by a chamfer sweep — a
  step sideways costing one, a diagonal root two — while a region grown
  by a distance used a true Euclidean one. The chamfer is wrong by up to
  a thirteenth, worst at an eighth of a turn: a band round a disc reached
  nearly a pixel less far there than along the axis, which is a circle
  drawn as an octagon. Both are the true distance now
  (`an_outline_round_a_disc_is_round`), and that is what gave the GPU
  backend an outline at all: a chamfer's passes each read what the one
  before wrote, which is a sequence, while the exact transform separates
  into a pass down each column and a pass along each row. Both renderers
  now hold the same reading of a band round a disc — painted out to
  50.00 and bare from just past it — and both tests fail with the
  chamfer put back.
- **Picking reads what a layer shows, not only its shape.** A mask and
  being held to the layer under it are both ways of a layer being
  somewhere it is not, and hit-testing used to look at neither: a shape
  masked down to a disc was pickable across the whole rectangle it was
  cut out of, and a layer held to a small one under it was pickable over
  the whole of itself. Both reach bare canvas, so clicking nothing picked
  a layer the pointer was nowhere near anything of. `shows_at` gates
  every kind now — the compositor's own coverage over the one pixel the
  point is in, so a feathered edge and a brushed mask are read the way
  they are drawn. What is *not* gated is deliberate and worth keeping
  straight: a copy is picked over the box its original occupies, since
  that is the box its handles are drawn round, and a block of text over
  its box rather than its glyphs.
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
     nothing of the node kinds — a clone layer was the last it had never
     drawn — and what it still hands a page back for is a thing a layer
     *holds* rather than the kind of layer it is: an outline wider than
     a pass will walk, and a copy of a clone layer wearing anything of
     its own (a mask, a fade, a blend, an effect, or being held). An
     effect on a clone layer came off that list (below). (A healing stroke
     was on that list; it is drawn now, below.)
     Two more came off in the same sitting — press ink, and a stroke
     carrying a region on a layer whose own mask was already riding that
     slot — and both are below. With those two gone there is no whole
     layer left that the shared fixture has to have removed before the
     cross-renderer audits can run: every one of its thirty-four is
     compared now. Asked command by command (October 2026), it draws all
     61 of the fixture's and declines none: the last, a mask on a frame
     that wears an effect, came off once it was asked directly (§0 *A
     filter inside a frame*), after clipping and effects, and turned
     frames, had (§0 *Clipping and effects, asked what is really
     declined*, and *A turned frame*). Asked by a tally of six thousand
     random pages rather than by what is written down (§0 *What the
     random pages really go back for*, October 2026), eleven declines
     came off in a row and the pages drawn went from 3081 to 5047 —
     among them a layer held to an adjustment, a blended layer's bevel,
     a picture with its corners moved, a frame drawn where it stands
     and mixed back, and two defects found on the way (a surface's way
     down cutting twice; the reference spending a held copy's blend).
     What is left is clone-shaped and frame-shaped: a layer held to a
     clone layer and a copy of a clone wearing anything (what the clone
     laid, which only the page has in hand), a frame wearing an effect
     or a blend while holding what reads the page (the frame drawn twice,
     once aside for its silhouette and once in place), a pixelate under
     a turned view, and the outline wider than a pass will walk.
     That list was three items longer an hour ago and every one of the
     three came off for a different reason, which is the argument for
     asking what is *really* refused rather than reading what is written
     down. Effects on a **group** and on a **copy** were drawn already,
     and the stale comment saying otherwise had been copied into this
     roadmap as work still to do. An effect on a **frame** was really
     refused, and the reason given for it was wrong; it is drawn now.
     And an effect on a **clone layer** was refused for a reason that
     applied to the reference renderer too, where it was silent rather
     than safe — that one was a defect, and it is below.
     A brush layer it draws
     (`a_brush_lays_the_strokes_the_cpu_lays`) — every stroke gathered
     into a coverage of its own with max blending, since the segments of
     one stroke union rather than pile up, then laid down in its colour
     or, for an eraser, taken off by a blend that subtracts it. A stroke
     laid inside a region it draws too
     (`a_stroke_laid_in_a_region_stays_in_it`): the region rides on the
     stroke rather than being read off the document, so the coverage is
     the stroke's own — one texture per stroke rather than one per layer,
     which is exactly what makes it the stroke's — riding the same slot a
     layer's mask does. One slot holds one coverage, so a layer whose own
     mask is already on it and whose stroke also carries a region is
     still the CPU's. A clone layer it draws too
     (`a_clone_lifts_what_the_cpu_lifts`), which is the last of the node
     kinds: it paints with what the surface already holds a fixed
     distance away, so it is never put on a surface of its own — on one
     there would be nothing under it to paint with — and its blend, its
     opacity and its mask go on each stroke as it lands, which is where
     the CPU renderer puts them. What it lifts and what it lands on are
     the same copy of the surface, taken before the stroke's pass, so a
     stroke running over its own source reads what was there rather than
     what it has just laid — and that is also what leaves the blend
     something to read. A clone layer wearing an **effect** it draws now
     too (`a_clone_layer_casts_its_shadow_here_too`), which meant
     putting one on a surface of its own after all — an effect wants a
     silhouette, and a clone's is what its strokes lay. On that surface
     each stroke reads two things (`fs_clone_aside`): the surface it will
     come down onto, still as it was when this one was opened, since
     everything drawn meanwhile went here, and a copy of what the strokes
     before it laid, the one seen through the other — the page with the
     earlier strokes on it, which is what the reference renderer lifts
     from as it draws the same layer aside. The strokes go down Normal,
     each faded as it lands, and the layer's blend and its effects come
     when the surface does, from the machinery every other layer on a
     surface already has. A heal on such a layer averages the same two
     through the same lens (`fs_heal_terms_aside`). Asked faded,
     blended, masked, healing, inside an isolated group (where what it
     comes down onto is the group's surface and the patch it lifts is
     on it) and wearing an outline and an inner shadow; five sabotages
     — reading only the parent, only what was laid, always the page as
     the parent, a heal's terms read the plain way, the fade left out of
     the silhouette — each fail it, two of them the random pages as
     well. Those pages went from 1377 drawn to 1477, against 1491 before
     they held clone layers at all. A healing stroke it draws too
     (`a_heal_averages_what_the_cpu_averages`), though its shift is an
     average over the whole stroke before any of it goes down — a
     reduction, and a pass of quads is not where one happens. So it gets
     passes that are not quads of the page: each pixel's share of the
     two sums onto a pair of full-precision textures the size of the
     stroke's rectangle, sixteen-by-sixteen sums of those until one texel
     is left, and the shift that texel says written beside the stroke's
     coverage. An adapter that cannot draw into a full-precision texture
     still hands a heal back.
     There is no arm left over in the walk now and none wanted, so a new
     kind of layer will not compile until it says how it is drawn.
     Effects on a *group* it draws too, and on a brush layer
     (`an_effect_on_a_group_is_built_from_what_it_composites`). Those two
     were out for the same reason and it is worth keeping straight: a
     layer that draws one thing has its opacity applied as it paints, so
     where its fill and its stroke overlap the fade is taken twice and
     that overlap *is* the silhouette — but a group's opacity belongs to
     the composite, so two overlapping children in a half-faded group
     make one half-faded shape with no seam down the overlap, and the
     shadow of it has none either. A brush layer is the same story, its
     strokes having their conversation with each other before any of it
     fades. So those two owe the silhouette their opacity at the moment
     it is built rather than having it inside the surface already, which
     is one number on the field pass. What is left out is a frame (the
     CPU renderer cuts its contents to its own rectangle before making a
     silhouette of them), a copy (what it draws is another layer,
     somewhere else) and a clone layer (never on a surface of its own to
     have a silhouette at all). Shadows it now draws,
     inner and outer, as the blur passes again read
     off the layer's own silhouette rather than off what is under it
     (`a_shadow_is_the_silhouette_the_cpu_casts`), on a leaf that is
     faded, masked or held to the one under it as well as on a plain one
     — all three decide what the silhouette is, so all three go into the
     surface the shadow is cast from rather than onto the quad that lays
     it down. Effects inside a frame it draws now
     (`an_effect_inside_a_frame_ends_where_the_frame_does`): a frame cuts
     what the layer *lays down* and not the silhouette its effects grew
     from — a shape half out of a frame casts the whole shape's shadow,
     cut at the frame's edge, rather than the shadow of the part that
     shows — and since a frame is whole page pixels, the cut is the
     rectangle the quads are drawn over rather than a coverage they read.
     That leaves the mask texture for the mask and leaves what was drawn
     into the surface uncut, which is exactly the distinction. A layer with a blend
     mode it draws too
     (`an_effect_on_a_blended_layer_comes_down_by_the_blend`): the CPU
     renderer brings each effect down by the layer's blend as well as the
     layer, and that wanted the one texture the stamp had already spoken
     for — a shadow is read at an offset and an inner one is held inside
     the silhouette, both of which want the layer's own coverage where a
     blend wants what is under it. So where there is a blend those two
     happen a pass earlier, on the scratch pair, and the stamp is left an
     ordinary picture to bring down.
     The fixture audit now compares all three of its layers with their
     effects on. Finding the third of them is what turned up a separate
     defect, in the stamp rather than the blend: the field was read
     *clamped* at its window's edge, which is nothing where the field is
     nothing there — but where the surface itself cuts the layer short
     the field is not nothing, and a shape hanging off the top of the
     page cast a shadow back onto the first row out of a silhouette
     neither renderer has
     (`an_effect_reads_nothing_where_the_surface_cut_the_layer`). An
     outline it draws too
     (`an_outline_is_the_band_the_cpu_measures`): its band is a true
     Euclidean distance from the silhouette — the same one a region is
     grown by — and that transform *separates*, so it is two passes here
     rather than a search of the whole disc: one down the columns for how
     far the nearest inside pixel in each is, then one along the rows
     taking the least of dx² + g², which is exactly the distance to the
     nearest inside pixel anywhere. Neither pass looks further than the
     band reaches, so the cap they are held to — a smear's, said in the
     same taps — can only decide an answer outside the band, where the
     answer is nothing. What the band is measured *from* is a yes or a no
     rather than a coverage, since an edge is where the silhouette is
     half covered — and half of the layer's *own* opacity at that, since
     a layer at a third opacity would otherwise have no inside at all and
     cast no outline. The
     shared fixture no longer has its effects stripped wholesale before
     the audit compares: they all come off and then each goes back
     wherever the page is still accepted with it there, so what the
     backend can draw stays in the comparison and what it cannot is out,
     asked rather than named. Both filters that used to be handed
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
     A *turned* rectangle thinner than a pixel was covered differently
     here, by up to 0.3, found while holding hairline outlines to the
     CPU: its coverage was a ramp a pixel wide across each axis, which
     is the overlap only while the rectangle is at least a pixel across,
     so a pixel whose middle sat inside a line a third of a pixel wide
     came out two thirds covered. Upright it never showed — the middles
     of the pixels such a line crosses lie outside it. Each axis is now
     what a pixel and the rectangle really share along it (`slab`),
     which is the ramp again for anything a pixel or wider
     (`a_thin_turned_rectangle_is_covered_by_its_area`). A slanted thin
     *path* still differs, by as much: a path is covered here by four
     samples, so a third of a pixel comes out a quarter or a half.
     Nothing in the fixture is that thin, which is why no audit said so.
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
     until a view did. What is left is for anything to *reach* for the
     backend, and that is bigger than it has been written here so far —
     worth stating plainly rather than leaving as a line item. The engine
     runs as WASM inside the webview on every platform today, the desktop
     shell included (`shells/tauri/src-tauri/src/lib.rs` is a window,
     menus and file dialogs and says so), and the UI is handed *pixels*
     over that boundary. So there are two honest routes and neither is a
     chunk:
     (a) compile this crate to wasm32 and run it on WebGPU in the
     webview, which means an async device bring-up in an engine whose
     render entry points are synchronous, wgpu in the app's wasm bundle,
     and a WebGPU-availability question per platform — the open half of
     Spike 1, still unanswered on mobile; or
     (b) run it natively in the shell and stop handing pixels across at
     all, presenting to a surface the shell owns, which is the bigger
     change to the presentation path but the one that pays on desktop
     first and does not need WebGPU anywhere.
     The spike's ~3ms round trip at 1280×720 on *software* Vulkan says a
     readback design would be fast enough, so (b) need not mean giving up
     the pixels-over-the-boundary shape all at once. **(b) was taken,
     as a readback** — see "GPU on screen" in the list above: the shell
     keeps a mirror of the document (`core/present`) and draws the frame
     the engine would have, opt-in, and the page draws its own wherever
     the mirror declines. What is left on this item is *measuring* it in
     a real shell on a real card (the container has neither), then
     presenting to a surface the shell owns rather than reading back, if
     the measurement says the copy is what costs; and (a) for the
     browser, still open — and asked again in October 2026: the crate
     compiles for wasm32 as it stands (wgpu's WebGPU backend comes with
     it), but the headless Chromium this container runs the smoke suite
     on offers no `navigator.gpu` at all, with or without
     `--enable-unsafe-webgpu` and a SwiftShader adapter, so nothing of
     (a) could be verified here, in CI or by eye, and it waits for a
     machine with a browser that has one, as the shell's measuring
     waits for a card. Holding the backend against the CPU renderer
     has turned up real defects on both sides — the clone stroke's lost
     region, and now the page's edge at the identity view — and the
     fixture audit
     (`whatever_the_gpu_agrees_to_draw_it_draws_the_way_the_cpu_does`) is
     what to run while doing any of it. See docs/spikes/gpu-rendering.md.
  2. Mobile shells ✅ as far as a machine without an account can take
     them: CI runs `tauri android init` and keeps an optimised,
     test-signed APK, and `tauri ios init` and keeps a Simulator build
     (Phase 4). What is left wants the owner: a kept Android signing key
     for a store build, and an Apple team for a device one.
  3. Depth. Nine methods have been paying, and all of them are cheap
     enough to keep reaching for. One: **put something in the shared fixture that
     nothing there has ever held** — an effect, a blend mode, a mask read
     off an image, a group two deep — and see which audits stop holding.
     That found a copy drawing its shadow clipped, turned up the
     clipping wrinkle above, and — once a layer in it reached for a
     palette entry — found a palette change repainting nothing and the
     GPU declining a page it can draw. Six shapes have gone in since. Three held: a
     copy of a *frame*, a copy of a *copy*, and a second frame. The
     fourth did not — a shadow on the *group*, where every effect in the
     fixture had until then hung on a layer — and it took two audits with
     it, one of which was a shadow that changed when the layer it
     belonged to moved near the page's edge. The fifth held: a layer
     *held to* the one under it, which is the one way of one layer
     deciding what another shows that the standing document had never
     had — and since holding proves little on its own, the code it was
     meant to exercise was broken to see who would notice. Somebody
     does: ignore the flag in the GPU backend and the cross-renderer
     audit fails on the bare fixture; drop it on the way to a file and
     the container audit says the clip did not survive. Which is the
     lesson applied rather than relearned.
     The sixth was a region *picked out* and a region *kept by name*,
     standing in the document rather than only appearing in the list of
     commands. Both had been in that list since they were written, which
     asks what each one does; neither had ever been in the document the
     rest of the audits are asked about, which is the other question —
     what a page with a region on it survives. One audit stopped holding,
     and it was the right one: the file format's inventory of fields a
     `.chitra` cannot be missing grew by twenty-six, all of them inside
     the selection and the kept regions, since a `Mask` is as required
     there as it is on a layer. Everything else held, so the code the
     addition was meant to exercise was broken to see who would notice:
     take the carry off the selection and three tests fail, off the kept
     regions and two do. Both were already pinned. Worth knowing rather
     than worth fixing — and the shared document is the stronger for
     standing with a region on it. Holding is not nothing — but the copy of a copy was
     worth more than that. Nothing broke, so the question became what
     would have to break for the audit to notice, and the answer was
     nothing: stopping the walk that finds copies of copies after one
     round left every audit green. So that is now asked directly
     (`changing_a_layer_repaints_the_copy_of_the_copy_of_it`), and the
     lesson is the method's own — when the fixture gains something and
     everything holds, break the code the new thing was meant to exercise
     and see whether anything notices.
     The seventh was a copy that *differs* from what it follows. Three
     copies had stood in that document since copies were written and
     every one of them drew its original entire; a copy with a layer of
     its own in place of one of the original's is the other half of what
     a copy is for — a badge drawn once and used twice with a different
     mark on the second — and nothing there had ever asked about it. It
     had to go beside a plain group rather than the pair, since the pair
     carries a shadow and a layer drawn as a whole cannot have its parts
     swapped, which is the rule the feature is built on.
     Every audit held, so the code it was meant to exercise was broken:
     make a copy ignore its own stand-ins and exactly two tests fail,
     both of them the ones written for the feature. Nothing else had ever
     looked. That sent the question to the exporters, and there it was —
     SVG and PDF both drew the original again whenever they met a copy,
     so a badge used twice with a different mark came out as two
     identical badges: a wrong picture in a file that reads perfectly
     well. Both now export what the copy *draws*, and a reader that is
     not us says so (`a_copy_that_differs_exports_what_it_draws`,
     `a_copy_that_differs_goes_over_as_what_it_draws`). Two more of the
     same family came with it: the box a copy is outlined by and the box
     it is picked over were both asked of the *original*, so a copy
     whose own layer reaches further than the layer it replaced was
     outlined short and could not be picked over the part that stuck out
     (`a_copy_is_outlined_and_picked_over_what_it_draws`). Four defects
     from one shape in the fixture, none of them reachable without a
     copy that is not its original.
     The eighth was an adjustment layer *inside a plain group*. Every
     adjustment and filter here had stood at the top of the page, where
     what it changes is everything under it; inside a group it changes
     its neighbours and nothing beneath, which is what makes a group be
     drawn on a surface of its own. It was put in the pair first, where
     everything held — and where it proved nothing, because that group
     already wears a shadow and so was already isolated for that. So it
     went into the one plain group the document has, where isolation is
     asked for by *what the group holds* and by nothing else. That is
     the shape that bites: stop the GPU backend isolating a group for
     what it reads and the cross-renderer audit fails on the bare
     fixture, where before the addition it passed. A decision both
     renderers make, and neither was being asked about it.
     The ninth was a mask *brushed by hand*. Two of the three kinds of
     mask had stood in that document — a shape and a picture — and the
     one with strokes of its own never had, which put a whole slot out
     of reach: a painted mask's strokes live where a paint layer's
     strokes live and are read by the same code, so it went on the paint
     layer and that layer now carries strokes on itself and strokes on
     its mask at once. Two audits stopped holding. One was the file
     format's inventory, which grew by the seven fields a stroke cannot
     be missing — the same three required of a paint layer's own, which
     is the right answer rather than a missing default. The other was a
     real defect, and a plain one: a mask is authored in its owner's
     *parent* space, a paste nudges what it pastes so the copy is not
     hidden behind the original, and nothing carried the mask. So a
     duplicated masked layer landed twelve pixels along wearing the
     original's mask and showing the wrong part of itself. Every kind of
     mask, since the space is the mask's and not the kind's — which the
     brushed one only *revealed*: the fixture's picture mask and shape
     mask sat on layers where the misplacement happened to draw the same
     picture, so the clipboard audit had been asking and getting away
     with it. A test asks all three now, each with a mask that shows
     some of the layer and hides some of it so that a mask in the wrong
     place cannot draw the same thing
     (`a_duplicate_carries_its_mask_with_it`), and the clipboard audit's
     field-by-field half expects the mask *carried* rather than the mask
     unchanged, since "the mask came over" was true of the broken
     version — it arrived intact and in the wrong place.
     The tenth was a stroke carrying the region it was painted inside.
     `PaintStroke::clip` is how a brush stays confined after the region
     is let go of, and no stroke in the shared document had ever carried
     one, so every audit that asks about a stroke had been asking about
     the easy half of what a stroke is. One audit stopped holding — the
     file format's inventory, by the twelve fields a region cannot be
     missing, a region being a mask and as required inside a stroke as
     on a layer. Everything else held, so the code it exercises was
     broken to see who would notice, and this time somebody did at every
     turn: stop carrying a stroke's region through a transform and two
     engine tests fail; leave the region unread on the clone path and
     the reference renderer's own test and the cross-renderer audit both
     fail; leave it unread on the brush path and the engine's test of
     the whole story fails. A test written for the brush path at the
     reference renderer's level was thrown away again on finding that
     out: it duplicated an engine test that covers more of the story,
     and a test kept for symmetry is a test that has to be read later.
     What the shape is worth, then, is not a defect but reach — a
     stroke's region now rides through the file round trip, the
     clipboard, the dirty-region audit and the undo runs, none of which
     had ever seen one.
     The eleventh was the other half of three commands rather than a new
     shape: `AddStroke`, `RemoveStroke` and `SetStroke` each say with a
     flag whether they mean the layer's strokes or its *mask's*, and the
     list only ever said the layer. A list like that looks complete —
     every variant is named, and the audit that checks the names is
     satisfied — while asking half the question of three of them. Now
     that the paint layer carries a brushed mask, all three are in the
     list both ways.
     Every audit held, so the code was broken to see who would notice,
     and the answer was exact: give the gesture's slot table a
     `Slot::Stroke` that forgets the flag, so a gesture rewriting a
     layer's stroke and its mask's records only the first inverse, and
     two audits fail. With the same sabotage and the list as it was, both
     pass. That is the addition earning its place rather than holding.
     And the paint layer no longer sits at the origin, which is a smaller
     thing with a sharper edge. A brush has two spaces in it — a layer's
     own strokes are in its own space, a painted mask's are in its
     parent's — and with the layer at the origin those two spaces are the
     same transform, so nothing could tell them apart. Compute a mask
     stroke's dirty region in the wrong one of the two and *still* every
     audit passed, which sent the question to why: the shared document
     carries a filter, and a filter's reach grows every dirty region in
     the document by its radius on every side, which swells a stroke's
     tight box to the whole page. Conservative, correct, and it swallows
     the error whole — a blind spot of that audit worth knowing about
     rather than fixing, since the conservatism is the right call. So the
     space has a test of its own on a page with no filter and a transform
     far larger than any rounding
     (`a_dab_on_a_mask_dirties_where_the_mask_says`), and it is the only
     thing in the workspace that fails when the two spaces are confused.
     The twelfth was a **path**, which is the largest thing this document
     had never held: every shape in it was a rectangle or an ellipse, and
     a path is what the pen draws and what a brush stroke becomes. Its own
     rasterizer, its own stroke — a skeleton walked with caps and joins
     rather than a band inside a closed outline — its own path data in
     both exporters, and none of it had ever been asked about here. Its
     stroke swells and tapers with it, since `Stroke::widths` is a width
     per anchor and only means anything on a path.
     Two defects, both of them the kind nobody files. The file format's
     inventory grew by two (a path's points and whether it closes; its
     smoothing, handles and subpaths are additive and read as absent),
     and then:
     The reference renderer **drew a swelling stroke at full width on any
     curve**. Per-anchor widths are indexed by the shape's own anchors, so
     a curved path — drawn as a polyline of far more points than it has
     anchors — needs them resampled onto that polyline, and
     `flatten_widths` does exactly that, while the anchors are still
     there. The draw path handed it a shape it had flattened first: more
     points than widths, read as "this stroke does not vary", full width.
     Every other caller — the hit test, the PDF exporter, the GPU backend
     — passed the shape itself already, which is why the GPU audit is what
     caught it. The test that existed used a straight two-point line,
     which flattens to itself and keeps the count
     (`a_stroke_swells_and_tapers_along_a_curve` is the case).
     And fixing that uncovered the second, in the browser suite: **adding
     an anchor to a brushed line fattened it**. The widths are indexed by
     the anchors and `insert_anchor` left the old list behind, so the list
     stopped lining up and the whole line went to full width in one
     double-click. It was invisible before because the line was being
     drawn at full width anyway. An anchor now takes the width the line
     had where it sits — between the two it was cut between, at the place
     along the segment the cut was made — and one taken off takes its
     width with it (`an_anchor_on_a_brushed_line_keeps_the_widths_lined_up`
     asks the widths rather than the picture, since the picture only says
     the line got fatter and not why).
     One threshold moved rather than an audit, and then the audit moved
     too. The JPEG round trip's mean went 3.0 to 4.0, a thin
     high-contrast diagonal being what JPEG rings worst on — measured at
     2.65 a channel away from the drawn line and 3.57 in the rows holding
     it. 2.65 was already most of the old ceiling, which is that
     instrument eroding as the page gains edges.
     It is a *worst block* now rather than a page mean. What JPEG loses is
     high frequency, and it rings inside the eight-by-eight blocks it
     works in, so averaging over one of those cancels most of the loss and
     leaves anything structural where it was — and a block's error is
     bounded by the contrast inside that block and by nothing else, so it
     does not drift when the page gains a line somewhere else.
     What is worth recording is that the change was nearly not worth
     making, and the measurement said so before the work did. Against a
     JPEG that is actually wrong — red and blue swapped, the channel-order
     defect the audit exists for — the page mean goes from about 3 to
     about 60 and the worst block from about 2 to 153. Sixteen times the
     room against forty: the old instrument was never close to failing,
     and the worry about erosion was right in principle and small in
     practice. The new one is kept because it is free, stronger, and
     drifts far less, not because the old one was failing. *Far less* and
     not *not at all* — the next thing added to this fixture said so, and
     it is recorded below.
     What it does not catch, and this is worth writing down beside the
     method: the same sabotage made to `reads_backdrop` itself is
     invisible to that audit, because the GPU backend asks the CPU's own
     function. Three engine tests catch it; no fixture audit can, and
     none of them is blind by accident — a shared answer is a shared
     answer, the same as with the walk that resolves a copy's stand-ins.
     The thirteenth was a **radial gradient**. Every fill in this fixture
     that was not flat was a linear one, and the random pages generate
     linear ramps too, so the disc — a different function of position,
     with its own centre and radius and its own way of being wrong — had
     never been drawn by anything the audits look at. It goes on the
     layer with the blend mode, so it is composited as well as drawn. The
     file format's inventory grew by nine (a radial's centre, its radius,
     its stops, and each stop's colour and offset), which is the point of
     that inventory: nine fields that were serialized and had never been
     round-tripped by anything.
     No defect this time, and the addition still earns its place, which
     is the part worth being careful about. Break the ramp in the
     reference renderer — measure the distance along one axis instead of
     the radius, a disc turned into a stripe — and with the gradient in
     the fixture *two* GPU audits fail: the one that compares gradients
     directly, and
     `whatever_the_gpu_agrees_to_draw_it_draws_the_way_the_cpu_does`,
     which is the fixture one. Take the gradient back out and only the
     first fails. So the fixture audit is newly able to catch a broken
     radial ramp, which is exactly what adding a shape to it is for: not
     a second opinion on a case already covered, but a case the
     document-wide audits could not previously reach. The render crate's
     own 164 tests, for the record, do not notice the sabotage at all.
     And it cost the JPEG instrument, one page after that instrument was
     rebuilt: the worst block went from 3.92 to 8.13, past a ceiling of
     6.0. The PNG check — lossless, held to a single level, and run
     before it — passed, which is what says the export is *right*; 8.13
     is JPEG's own loss on a smooth sweep through saturated hues, which
     is a hard case for it in a different way than an edge is, and which
     a block average does not cancel the way it cancels ringing. The
     ceiling is 12.0 now, against 153 for a JPEG with its channels
     swapped. The honest summary is the one now written into that test:
     a worst block reduces the drift rather than ending it, and the
     thing that says an export is correct is the lossless check above it.
     The fourteenth was a stroke lying **outside** its outline, and it
     came out of a different way of choosing what to add: ask the
     document which of its own vocabulary it has never used. Serialize
     the fixture, collect every key and every string in it, and hold that
     against what the crate declares. Every *field* was there — which is
     itself worth knowing, and says the previous thirteen did their job.
     Every *enum variant* was not. `StrokeAlign` has three and the shared
     document held none of them: not by choice but because `align: None`
     means whatever a shape has always been stroked as, and for a rect
     that is inside. A gap of that shape is easy to miss, since nothing
     is unset — the fallback is a real answer and every audit had been
     given it.
     Outside is the variant that changes more than colour: `stroke_pad`
     grows the layer's box by it, so the dirty region, the hit test and
     any effect's silhouette move with it, and it is the band SVG and PDF
     have no way to ask for — both draw it as a centred stroke on a shape
     pushed half a width outwards.
     No defect in the drawing of it, and the addition earns its place
     anyway. Take `stroke_pad` down to nothing for an outside band — the
     box that no longer grows, while the band still draws — and with the
     stroke in the fixture two audits fail that do not fail without it:
     the cross-renderer one, and `a_layer_dragged_into_another_group_
     brings_its_mask`. The dedicated stroke tests catch the drawing;
     nothing but the fixture caught the box.
     Worth recording alongside: the first sabotage tried was the wrong
     one, and saying so is the point. `stroke_pad` for an outside band
     was taken from twice a width to once — and the whole workspace
     passed. That looked like a gap for a minute. It is not: the pad is
     deliberately generous (a centred band reaches half a width and is
     padded a whole one), so once a width is still correct for an outside
     one, just exact. A sabotage that leaves the code right proves
     nothing, and the way to tell is to read what the thing is *for*
     rather than to trust that a changed number is a changed meaning.
     And it found an unstated precondition in an audit one half-pixel
     from being violated. The clipboard audit draws a layer and its
     pasted copy and holds them against each other with the nudge allowed
     for — "the copy draws what the original draws, moved". That is only
     true where the page did not cut the original short. An effect is
     built from the layer's silhouette over a window and the page's edge
     is where that silhouette stops, deliberately, so a layer whose
     effect window runs off the page is genuinely a different picture
     from the same layer moved inwards. The fixture's lower shape already
     reached half a pixel past the left edge — under the threshold, and
     so the assumption held by luck. An outside band pushed it to two and
     a half. The comparison now excludes a band as wide as the effect
     reaches on the sides the window actually crossed, which is nothing
     at all for a layer sitting clear of the edges; a paste that lands
     half a unit off is still caught, in the middle of the page, at
     0.46 of a channel.
     A fifteenth shape was tried and thrown away, and what it turned up
     on the way out was worth more than the shape. The list from the
     method below named `StrokeJoin::Miter` and `StrokeCap::Square`, so a
     polyline with real corners went into the fixture — the only path
     there is `smooth: true`, flattened into so many nearly-straight
     segments that the join between two of them is barely a join. Three
     sabotages later it came back out. No audit over the shared document
     can use a corner: the export witnesses compare layer *interiors*,
     which is a pixel opaque with eight opaque neighbours of its own
     colour, and a join is all edge and taper; the GPU backend hands a
     mitred path back rather than drawing it; and a runaway point is cut
     to the layer's own box by `stroke_pad` before it can reach anywhere
     a page-wide comparison would find it. The shape held, and holding is
     not earning.
     What it turned up is that **the miter limit had no test at all**,
     though there appeared to be one. `a_join_says_how_the_line_turns`
     ended with a check that a corner past the limit is cut off rather
     than carried out — and that check could not fail. Three things were
     wrong with it at once and any one would have been enough: it counted
     inked columns in `60..100` while the corner was at 60 and the spike
     points the other way, so it counted a region nothing ever drew in;
     its bound was `MITER_LIMIT * 12.0`, forty-eight, over a range only
     forty wide, so the count could not reach it; and it asked for ink at
     more than half coverage, which a mitre's taper never has at its far
     end. Undo the limit entirely — let a corner of ten degrees keep a
     point it should lose — and the whole workspace passed.
     It asks the furthest ink now, at any coverage, on a page and a width
     of its own chosen so the limit lands well inside the page: 36 with
     the limit in force and 52 without it. Which is also where the
     `stroke_pad` fact came from, and it is worth keeping: a point let
     out past the limit is not unbounded ink, because the pad grows the
     box by `MITER_LIMIT` half-widths and the renderer cuts to it — so
     the wrongness is a spike sliced off square rather than a spike
     running away. That is why it has to be measured against where a
     *bevel* would end rather than against the page.
     And a second, in the SVG exporter: it wrote no `stroke-miterlimit`
     at all. Miter is SVG's own default so the join needed no attribute,
     and SVG's default limit is 4 and `MITER_LIMIT` is 4, so the picture
     came out right — by a coincidence holding two numbers together
     across a file format with nothing watching it. Change the constant
     and every mitred corner in every exported SVG disagrees with the
     engine, silently, and no witness can say so for the reason above.
     The PDF exporter had always written its own limit (`M`); this one
     says it now too, so the two agree because they both say the same
     number rather than because neither had to. Asked of the markup
     rather than the picture, which is method Two and is the only way
     this one can be asked.
     The sixteenth was **a copy wearing a mask**, and it found nothing it
     was aimed at and something much worse a foot to the left. Four copies
     had stood in this document since copies were written and every one of
     them is bare — no mask, no fade, nothing of its own — so the whole
     question of what a copy's own mask means had never been asked here,
     and it is the question that had just produced three defects in two
     renderers. Every audit held. So, the method's own rule, the code it
     was meant to exercise was broken: make the reference renderer ignore
     a copy's mask entirely and see who notices. Six tests do — two over
     pages nobody wrote, four in the GPU crate, one of them the new
     reading below. No exporter and no file round trip does, which is
     worth knowing on its own: a copy's mask is a thing only a renderer
     has ever been asked about.
     **That count was first read as one, and the reading was the
     instrument's fault rather than the suite's.** `cargo test --workspace`
     stops at the first test binary that fails, so the render crate failing
     meant the GPU crate never ran at all — `--no-fail-fast` is what makes
     "who notices" a question about the suite rather than about the order
     the crates happen to build in. Every sabotage from here on is run that
     way. The wrong count went out in a commit message and is corrected
     here, which is the second time this session a conclusion has come
     from a measurement that was not measuring what it was read as; the
     first was a sweep whose filter, not whose fix, was moving the
     number.
     Asking why turned up the thing worth having. That audit's reading is
     a mean over the whole page against 0.004, and **nine of this
     fixture's thirty-four layers can be removed outright without moving
     it**: the picture, the text, the held-to layer and the group holding
     it, the masked adjustment, the adjustment inside a group, the clone
     layer, the copy's stand-in, and the new copy. Several of those are
     shapes put here in earlier rounds precisely so this audit would
     compare them. The text block moves the page by 0.00003 — three
     orders of magnitude clear of the threshold.
     The interior reading is no help, which is the part worth knowing.
     It is the sharpest instrument here and it exists to tell a drawing
     apart from its antialiasing, so it needs a pixel with eight
     neighbours of its own colour — and a glyph has none. Drop text from
     the GPU backend altogether and the interiors read 0.0004 with
     nothing over the threshold, the page mean reads 0.00134, and both
     assertions pass. The whole of text rendering, on every audit this
     document has ever carried, was being compared on twenty-five
     antialiased pixels.
     Twenty-five because the text was also *falling off the page*: an
     18pt line written at a baseline of 54 on a page 60 tall hangs most
     of itself below the edge, and the raster mask meant to cut it sat at
     y 50 to 58, under the glyphs rather than over them. Moved up, the
     line puts down 115 pixels with the mask still cutting a third of it.
     A fixture shape nothing can see is not a fixture shape, and this one
     had been unreadable since it was written.
     What replaces the blind spot is a reading that cannot be diluted:
     **what a layer puts on the page**, taken as the difference between
     the page with it and the page without it, on each backend, and the
     two compared (`every_layer_of_the_fixture_puts_down_what_the_cpu_puts_down`).
     A layer shows up at its own size rather than the page's, however
     small it is and however much the fixture grows around it. Allowing a
     tenth of a layer's own inked pixels to differ leaves room for
     antialiasing along an edge and none for a layer to go missing: on
     correct code the worst layer reads 28 of 337 allowed, and with text
     dropped `words` reads 45 of 116 against 11. The whole-page mean saw
     nothing; the interiors saw nothing.
     The old audit gained the interior reading too, which costs nothing
     and catches the wide errors the mean would dilute.
     And the shape earns its place after all, once the sabotage is run
     properly: the new reading is among the six that catch a copy's mask
     being ignored, and this copy is the only masked one in the document,
     so without it there would be nothing there to catch. What was
     genuinely blind was text, and blind in the way that matters — with
     the fixture as it stood, dropping text from the GPU backend entirely
     passed the whole-page mean at 0.00134 and the interior reading at
     0.0004 with nothing over the threshold, so the audit meant to compare
     the two renderers over a page with one of everything on it had no
     objection at all to one of the everything going missing. Tests
     written *for* text do catch it; that is not the same thing.
     **A frame with an effect opened a surface nothing ever closed.**
     The reason written down for handing such a page back was that the
     silhouette would be "the surface uncut" — a child sticking out past
     the frame's edge casting a shadow the frame's own rectangle would
     not. That reason is wrong, and checking it took one page: a frame's
     contents are collected with its rectangle as their bound, so the
     surface holds them already cut, and the shadow is the frame's. The
     reference renderer agrees, and does so whether or not a child
     overhangs.
     What was really wrong was plainer and had nothing to do with
     silhouettes. The frame's arm of the pass returns before the end, and
     the surface is brought down *after* it — so opening one for a frame
     left it open. No shadow anywhere, and the frame's own pixels wrong
     besides: the backend's "what the effect adds" landed on the frame's
     own rectangle rather than five pixels down and across from it.
     Nothing was ever wrong on anybody's page, because the page was
     declined; but it was declined for a reason nobody had checked, and
     the check is four lines.
     The test's sharp assertion is the overhang, which is what the old
     reason was about: with a child half again the frame's size inside
     it, the shadow has to be the same shadow as with no child at all, on
     *both* renderers. Break the cut and it fails on the overhang case
     alone, which is the assertion doing exactly the work it was written
     for.
     **A shadow on a clone layer did nothing at all, and nothing said
     so.** Found by asking the previous question — what does this
     backend still refuse, and why — rather than by looking for a defect.
     Two of the five things on that list turned out to be drawn already;
     of the two really refused, a clone layer with an effect was refused
     for a reason that *also applied to the reference renderer*, and
     there it was silent rather than safe.
     An effect is built from a layer's silhouette, and a clone layer has
     no surface of its own to take one from: what it paints with is what
     is under it, and a fresh surface has nothing under it. So it is
     drawn where it stands, among the adjustments and filters — and
     those two really have no silhouette, being changes to what is below
     rather than pictures. A clone layer is not like them. It lays
     strokes, and strokes have a shape. The renderer's own `effected`
     predicate said as much and was not being listened to: it excludes an
     adjustment and a filter *by name* and lets a clone layer through,
     and then the branch below it swallowed the layer before any effect
     could be drawn. A predicate disagreeing with the code under it is
     the tell that this was an oversight rather than a decision.
     The silhouette costs nothing to have: `draw_clone` already works out
     what it lays, pixel by pixel, and now writes it aside as it goes.
     The layer is drawn once into a scratch page, where its reads see
     exactly what they would have seen; the effects that go under are
     drawn from what it laid; then what it laid comes down on top of
     them, with the layer's blend taken once over the whole of it rather
     than stroke by stroke — which is what every other kind with an
     effect on it gets, and for the same reason.
     The test asks the sharp question rather than the easy one. A clone
     layer's silhouette is its strokes at the alpha it lifted them at, so
     where it clones from something opaque it is *exactly* the silhouette
     a paint layer with the same strokes would have, and the two shadows
     must be the same shadow. Asserting that some shadow appears would
     pass on a silhouette of the wrong shape — and the second sabotage
     shows it: leave the silhouette unwritten and the shadow covers 111
     pixels where the brush layer's covers 382.
     Its first draft was vacuous in a way worth recording, since it is
     the third vacuous draft this session: it cloned from a *uniform
     ground* onto itself, which puts back exactly what was already there
     and lays nothing. Zero pixels, read as the defect rather than as the
     page. A clone layer needs something worth lifting or it has no
     silhouette at all, and the non-vacuity floor in the test now says so.
     The backend still hands such a page back, and that refusal has gone
     from safe to *necessary*: before this there was no shadow for the
     two to disagree about. `an_effect_on_a_clone_layer_goes_back` pins
     it, and asks the two things that keep a limit honest — that the
     effect really changes the picture, and that the same layer bare is
     still drawn.
     **Ink authored for a press was refused for half a reason.** Asked
     of the same list one more time, and the answer was written down in
     the backend's own module doc: a colour authored in CMYK "resolves
     through the document's profile, which is the CPU's business". The
     first half is true. The second does not follow, and the code one
     function away already knew it — the tint of a shadow accepted press
     ink whenever the document had no profile, which is exactly the case
     where the two renderers would have agreed.
     Resolving an authored colour is one question per colour with one
     answer per document. A fill is one colour; a stroke is one colour; a
     gradient's stops are resolved once into a ramp of 512 texels either
     way; a block of text is one colour; a shadow's tint is one colour.
     None of it is per pixel, so none of it needs a profile on the GPU at
     all — it needs the CPU's *answer*, which is what
     `chitrakar_render::resolve_color` is (public now, for that reason).
     Both renderers call it, so the two cannot drift by construction
     rather than by agreement.
     What that cost was two guards and two functions losing their
     `Option`. What it bought is every place a colour reaches a page:
     `ink_authored_for_a_press_lands_where_the_cpu_lands_it` puts ink in
     all six of them — a fill, a stroke, a stop in a ramp, a block of
     text, a shadow's tint, and a swatch whose *name* means an ink — and
     asks the page twice, once with no profile and once through a real
     one. The second run asserts the profile **moved the picture** before
     it asserts the two agree about it; without that the whole profiled
     half would pass on a backend quietly ignoring the profile, since two
     device-formula answers are identical. Made to ignore it, exactly one
     test in the workspace fails, and it is this one.
     It also mended a divergence that was latent rather than live: the
     backend's effect tints went through `chitrakar_color::to_working`
     and so past the profile, held harmless only by the guard that
     refused the page whenever a profile existed. Both halves now come
     from the same function.
     That left the fixture's paint layer out of the cross-renderer audit
     for one remaining reason rather than two, and checking *which* is
     what produced the next find. Not press ink any more but the one
     limit left in the walk: that layer wears a brushed mask *and* its
     strokes carry the region they were painted inside, and one texture
     slot holds one coverage. Checked rather than assumed, the way the
     frame's reason should have been — take the mask off and the page is
     accepted; take the stroke's region off and the page is accepted;
     leave both and it is not.
     **And then that limit went too, and the interesting part is which
     half of it was real.** One texture slot holds one coverage: a
     layer's own mask rides it, and so does the region a stroke was
     painted inside, so a layer with both handed the page back. Two
     coverages read together are one coverage — which is the answer this
     backend has always given a layer that is both masked and held to
     another — so they are multiplied into the stroke's own texture now,
     and `held_back` is the one place that folds a mask, a held-to layer
     and a frame's rectangle together, asked once for a layer and once
     for a stroke so the two cannot come to disagree about what held
     back means.
     The first sabotage said the fold changed nothing: dropped entirely,
     the whole workspace passed. That is the instrument again and not the
     thing — the case it serves is a **clone** layer, and nothing was
     asking about one. A paint layer is always drawn on a surface of its
     own (its strokes have their conversation with each other before any
     of them meets the page), so its mask goes on the quad that lays that
     surface down and never met the stroke's region at all: for a paint
     layer the refusal had been over-broad rather than load-bearing. A
     clone layer is never on a surface of its own — what it paints with
     is what is under it — so its mask rides each stroke, and there the
     collision was real.
     Which turned up the defect the fold would otherwise have introduced.
     Folded everywhere, a masked paint layer whose stroke carries a
     region takes its mask **twice**: once in the stroke's texture and
     once on the way down. A hard-edged mask hides that completely, being
     idempotent; a feathered one does not, half a coverage squared being
     a quarter. So the fold is conditional — nothing where the mask goes
     down with the surface, everything where it rides the layer's own
     drawing, which is a clone layer always and a paint layer with
     effects (there the mask belongs to the drawing, because it decides
     what silhouette a shadow is cast from).
     Three sabotages, three tests, one each: drop the fold and only
     `a_clone_lifts_what_the_cpu_lifts` fails; drop the pass that puts a
     stroke's own coverage back after the layer's has been written over
     everything, and the same one fails; fold where the mask was already
     going down with the surface, and `a_stroke_laid_in_a_region_stays_in_it`
     fails on its feathered case, which is there for exactly that.
     The clone reading is laid out so each coverage owns a side of the
     dab and they share the middle — the mask everything left of x = 56,
     the region everything right of x = 44, the dab reaching past both —
     with hard edges, since what is being asked is *which* coverages were
     read. And it is read on both renderers rather than compared between
     them, so two backends losing the same coverage cannot agree their
     way past it.
     One wrong turn worth recording: the first version of that reading
     used a feathered disc for the mask and read three points, and the
     non-vacuity check failed — the case drew nothing where it should
     have drawn. That looked like a defect for a minute. It was the
     geometry of the test: a feathered mask over a soft dab leaves very
     little at full strength, and the points chosen were in the part it
     had faded. Rebuilt with hard edges the same three points answer
     unambiguously, and both renderers agree with each other *and* with
     what the arithmetic says. A test whose failure you cannot explain
     is not a finding yet.
     The twentieth shape came from the vocabulary sweep rather than from
     a hunch — serialize the fixture, collect every string and key in it,
     and hold that against the enum variants the crate declares — and the
     answer was **thirteen of sixteen blend modes**. This document had
     Normal and the Multiply on the layer inside the pair, and that looks
     like a gap of degree until you notice that four of the thirteen are
     not the same arithmetic at all. A separable blend is one function of
     one channel and its opposite number, run three times. **Hue,
     Saturation, Color and Luminosity** take one of brightness, hue and
     saturation from the layer and the rest from what is under it: their
     own code in both renderers (`set_lum`, `set_sat`, `clip_color` and
     W3C's own luminosity weights), their own names in both exporters,
     and their own case in the shader. Neither instrument had ever drawn
     one — `BLENDS`, the list a page nobody wrote picks from, was six
     separable ones.
     Hue of the four in the fixture, because it is the only one that
     reaches all of the machinery: `set_lum(set_sat(s, sat(b)), lum(b))`
     uses the saturation transfer where Color and Luminosity use only the
     brightness one. Laid over the pair, since a non-separable blend
     against a grey backdrop is a blend that does nothing. And all four
     added to `BLENDS`.
     Every audit held, so the rule: break the code it was meant to
     exercise. Make `set_sat` the identity and five tests fail — two that
     were already there (the render crate's reading of the spec, the GPU
     crate's blend-by-blend one) and **three that are new reach**: both
     fixture audits and the random pages. So the shape adds reach rather
     than an assertion, which is worth saying plainly.
     What it turned up is in the other instrument, and that is the find.
     **The resvg witness page carried no blend mode at all.** That test —
     "the last word on export fidelity", a reader that is not us drawing
     the whole page — had never once checked that a blend survives the
     export. All sixteen were pinned by reading the markup back and by
     nothing else, which is the same shape of blind spot as SVG text
     alignment, recorded a few shapes ago as something a second witness
     would close.
     So the witness page has a Luminosity layer now, over the flat red
     rect and clear of every edge and every spot check. It holds — which
     is itself worth knowing twice over: resvg implements the
     non-separable blends, and it agrees with the engine, which is an
     independent check on the decision that a blend is taken in the
     encoding a device shows rather than in linear light. Taken in linear
     light the same pixel would be a different pink.
     And it earns its place: export Luminosity as `normal` and **two**
     tests fail now where one did before — the markup one and the
     witness. Belt and braces, for blends at least; text alignment is
     still pinned by the markup alone.
     The spot check is written down rather than derived, at [255, 164,
     164]: a reader that did not implement the blend would put the
     layer's own pale yellow there, and one that took it in linear light
     would put a different pink, so the number tells those two apart.
     The next item off that same sweep was **five of six filters**, and
     it paid the way the method says it should: a page nobody wrote drew
     one of the four missing ones and two renderers stopped agreeing.
     `BLENDS`' neighbour in that generator offered a blur or a pixelate
     and nothing else, so a sharpen, a smear, a vignette and grain had
     never been drawn by either instrument. All six now, and each is a
     different shape of question — a sharpen gives back more than it was
     given and so is the one that can put a value above white; a smear
     reads along a line rather than across the axes; a vignette and grain
     are functions of *where a pixel is* rather than of what is under it.
     **A pointwise filter was measured from the surface rather than from
     the layer's own space.** The reference renderer asks
     `Inverse::of(view).at(x + 0.5, y + 0.5)` and the backend read the
     page pixel, and while the view is the identity those are the same
     number — which is why this stood. A **copy** is where they part
     company: it draws what it copies somewhere else entirely, so the
     view it is drawn under carries the copy's placement. Seed 617 is a
     copy of a noise filter, and the reference moved the grain with the
     copy while the backend left it pinned to the page: 0.075 mean,
     0.363 at the worst pixel, where the ceiling is 0.004.
     The inverse travels on the quad now and is read in the shader the
     way `Inverse::at` reads it, arithmetic for arithmetic, because a
     grain cell is a `floor` and a last bit rounding the other way puts a
     whole speck in the next cell. It cost a little packing: the quad's
     four spare numbers went to the inverse, so the seed's low half moved
     into the third of the `extra` slots and *mono* stopped being a flag
     and became the kind — a page's grain being one colour or three is as
     much a different filter as a vignette is. And the page uniform's
     spare `vec2` now carries the document's own size, which neither the
     surface nor the page rectangle says once a view scales anything, and
     which a vignette needs: it is placed from the page's middle in page
     units.
     The other half of the same defect had no instrument at all. The view
     audit's page held no pointwise filter, so a viewport — the thing
     this backend exists to serve one day — had never once been asked
     whether a filter lands where the reference puts it. It has a
     vignette now, and measuring it from the surface fails that audit and
     nothing else in the workspace.
     `a_copy_of_a_filter_carries_where_the_filter_is_measured_from` pins
     the copy half, and its sharp assertion is not that the two renderers
     agree — they would agree on the grain being pinned if both pinned
     it. It is that **on the reference renderer the copy's patch is the
     original's patch moved**, which is what a copy means, and then that
     the backend draws that same page. Grain in one colour, grain in
     three and a vignette, each with a floor saying the copy really put
     something on a page that had nothing there.
     One threshold moved and the honest account is that it had to.
     Extending the generator reshuffles every page it makes — a `Noise`
     draws more numbers than a `GaussianBlur` — so these are two thousand
     *new* pages, and exactly one of them, seed 1529, sits over the page
     mean's 0.004, at 0.00737. Run to ground rather than tolerated: it
     holds a self-intersecting path whose long thin wedge is nearly all
     edge, under a Difference blend against a strongly contrasting
     ground, then sharpened — which multiplies a difference by one and a
     half — and then given more contrast. That path *alone* on the same
     ground reads 0.00103. So the number is four samples a pixel against
     an exact area, put through three amplifiers, and the interiors
     reading — the one that tells a drawing apart from its antialiasing —
     is clean on all two thousand, with nothing over its point threshold
     anywhere.
     What makes the move safe is that the **count** is now held rather
     than the level alone: at most two pages of two thousand may sit over
     0.004. A change that makes twenty pages drift a little is caught
     there even though each stays well inside the ceiling, which is a
     stronger statement than the single number it replaces. The worst
     pixel anywhere moved 0.607 to 0.659 and it is the same page and the
     same reason.
     The twenty-first shape was **grain in the shared document**, and it
     is worth recording as two results rather than one, because they
     point opposite ways.
     What it was aimed at, it missed. Six kinds of filter and this
     document held a blur — and a blur is a neighbourhood, which the
     shadows already ask about. What none of these audits had asked is
     whether an answer that depends only on *position* is the same
     answer every time: a page redrawn a region at a time has to grain
     exactly as the page drawn whole, since a cell is a `floor` and a
     boundary read from the region's corner instead of the page's puts a
     speck in the wrong square. Measured the only way that settles it —
     make the reference renderer read the cell from the region's corner
     — and exactly one test in the workspace fails, and it is
     `a_region_render_padded_by_the_reach_is_the_page`, which already
     ran every filter kind including this one over a page of its own.
     The engine's own repaint audit does not notice, for the reason
     already written down beside it: this document carries a filter, and
     a filter's reach swells every dirty region to the whole page, so the
     region-wise path is never asked a partial question here. That
     conservatism is the right call and it swallows this whole.
     What it earns is on the other renderer. Shift the backend's grain by
     a single cell and **five** tests fail with the shape in, **three**
     without it — and the two it adds are both of the fixture audits, so
     a page with one of everything on it can now see a grain cell in the
     wrong place, which it could not before.
     Getting that second number right took two goes, and the first was
     the instrument again: the shape was taken out by removing the node
     straight after adding it, which left the `SetMask` that follows
     pointing at nothing and changed the document not at all — so the
     failure set came back identical and read, for a minute, as "the
     addition earns nothing". Hiding the layer instead is what asked the
     question. Fourth time this session that a measurement had to be
     checked before its answer was believed.
     One audit stopped holding on the shape alone, the usual one: the
     file format's inventory, by the four numbers grain cannot be missing
     — `amount`, `grain`, `mono` and `seed`, none of which has a default
     anybody could pick, since a file missing any one of them is a file
     that grains differently.
     Then the question the filters raised: **what do the exporters say
     about one?** They disagree, and had always disagreed. PDF, meeting
     an adjustment or a filter, renders everything up to and including it
     into one picture and places that — so the grade is in the file. SVG
     wrote an XML comment and left the layer out, so the same document
     exported the other way came out *ungraded*. A wrong picture in a
     file that reads perfectly well, which is the shape of defect this
     roadmap has recorded twice before.
     It was a decision rather than an accident — there is a test
     asserting the comment — and the reason given is true of five of the
     six filters and of all thirteen adjustments: SVG has no way to say
     them that composites the way this does. It is not true of a **blur**.
     `feGaussianBlur` is the same two-dimensional gaussian on the same
     premultiplied values in the same linear light, and "changes what is
     under it" is, in SVG, a `filter` on a group holding the siblings the
     layer follows. So a plain blur is exported now — and plain means
     plain: faded, masked, held to the layer below, blended or wearing an
     effect of its own and it is a comment again, because there is no
     primitive that means *half* of this.
     Two things had to be got right and the second is the interesting
     one. The filter region: SVG's default is a box a tenth larger than
     what is filtered, which cuts a blur off well inside its own spread,
     and `userSpaceOnUse` is the *current* space, which inside a group is
     that group's own — so the region is worked out from the bounds of
     what is being softened, in the space it is placed in, grown by how
     far the blur reaches.
     And the number: **`stdDeviation` is not the sigma the document
     asks for.** Both ends approximate a gaussian by three box passes and
     neither chooses its box the same way — the W3C runs two boxes of `d`
     and one of `d + 1` where `d` is even, this renderer halves `d` and
     runs three of `2r + 1` — so a sigma written through unchanged is a
     blur of the right name and the wrong width: at sigma 2 this renderer
     spreads by 2.449 and a reader by 2.121, and the pages do not match.
     What travels instead is the spread this renderer *actually has*:
     three boxes of width `w` have variance `3(w² − 1)/12`, so the blur it
     performs is one of `√(w² − 1)/2`. That lands right on both kinds of
     reader — one doing the spec's box passes picks `d = w`, odd, which
     is three boxes of `w`; one doing a true gaussian does the amount of
     blurring this actually is.
     What is left after that is irreducible and is written into the
     witness rather than glossed: a box-cubed and a true gaussian are
     different *shapes* at the same spread, so the softened patch lands
     **7.27 channel levels** from the engine's own where the rest of the
     page is inside one. The page mean went 3.30 to 3.75 and its ceiling
     3.5 to 4.5. That loosening is exactly what that instrument's own
     note says will happen, and it is not left to carry the claim: the
     softened patch has a reading of its own, taken first, and it is the
     one that bites — with the blur omitted again it reads 15.76 against
     a ceiling of 9, where the page mean only goes to 4.56, a hair over
     rather than an answer.
     Both numbers were measured in both directions before being written
     down, which is worth saying because the first draft of that comment
     guessed "fifty" for what turned out to be 15.76.
     And the markup is pinned beside the picture, as the blends now are:
     that a blur is not omitted, that its `stdDeviation` is √24⁄2 rather
     than 2, that it is applied to a group holding what it softens, and
     that half a blur is still a comment.
     What is *not* closed is the asymmetry that started this: an
     adjustment layer, and five of the six filters, still leave an SVG
     export ungraded where a PDF export carries them as pixels. Closing
     it means SVG learning to rasterize through them the way PDF does,
     which is a product decision — a vector file that is partly a bitmap
     — rather than a defect to fix quietly.
     A different question, and the first one here about **time** rather
     than about what is drawn: how does this scale? Nothing had ever
     asked, and the answer was that **adding a layer to a document cost
     the whole document**. Building 1600 layers took **2.23 seconds**,
     and 2.22 of those were two walks.
     `Document::apply` runs two checks after any command that could nest
     something — is there a cycle among the copies, and do the layers
     nest past `MAX_DEPTH` — and both walked the whole tree every time.
     N commands, O(n) each: a drawing that grows gets quadratically
     slower to build, which is not a slow editor but one that stops
     working as the work gets big. It is invisible on the documents a
     test usually builds, which is why nothing had seen it.
     Both checks still refuse exactly what they refused, and both are
     narrowed by the same argument: *nothing that has been applied
     breaks them* — that is what they are for — so after a command the
     only thing that can be broken is what that command changed.
     The **cycle** walk starts from the layer whose arrival made the
     edge rather than from the root. Every edge a command adds is
     incident to one node — the layer that arrived, moved, or was told
     to copy something else — so a cycle running through a new edge runs
     through that node, and a cycle running through no new edge existed
     before and is ruled out by the invariant. A batch and a restored
     subtree still take the whole walk: both can make several edges at
     once and both are rare enough that narrowing them would be
     reasoning nobody could check.
     The **nesting** check is skipped while the document is known to be
     shallow. A layer arriving adds exactly one level under the group it
     joins, so the tree grows by at most one per `AddNode`; an upper
     bound is kept, a walk resets it to the truth, and `None` — which is
     what a document read from a file has — means walk, because a bound
     that is not an upper bound would skip a check that matters.
     Measured: 1600 layers, 2.23s to 5.91ms, and **linear** rather than
     quadratic (801µs, 1.24ms, 3.29ms, 5.91ms for 200, 400, 800, 1600).
     The cycle walk was 87% of it and the nesting walk the rest.
     `adding_a_layer_costs_one_layer` guards the shape rather than the
     speed: four times the layers may cost four times the work and not
     sixteen, so a slow or busy machine moves both numbers together. It
     has a floor as well as a ceiling, since a build that took no time
     at all would satisfy any ratio.
     And the narrowings needed watching, which is where the real gap
     was: **`TooDeep` had no test at all**. The limit was written down,
     the check ran after every command, and nothing anywhere asked
     whether a document could be nested past it. So the limit is reached
     here a group at a time, which is exactly the path the new bound is
     on — and a leaf at the bottom is refused too, since what the limit
     counts is levels rather than groups, and a layer beside the chain
     is still accepted, which says the refusals are about the limit and
     not about groups having stopped taking children. Let the bound
     drift upward without ever forcing a walk and two tests fail.
     The cycle check had two tests and neither covered `SetKind`, which
     is the third way an edge between copies is made and the only one
     that makes it without moving anything — so it is precisely the one
     a check watching where layers *arrive* could miss. It has one now;
     turn the detection off and four tests fail, that one among them.
     What is **not** fixed, and is the next thing here: a `Session`'s own
     per-command cost is still O(n). The same 1600 layers built through
     one take 604ms where the document alone takes 5.91 — so the dirty
     tracking is doing a walk of its own, and it is now the whole of what
     is left.
     Which is what the next round did, and the session's two walks came
     off for the same reason the document's did: both are questions
     about the *whole document* that the command can answer by itself.
     Does anything here read a neighbourhood — a filter, a clone layer —
     and is there a copy on the page? A layer arriving says whether it
     is one, and whatever was there is there still, so only a command
     that can take the *last* one away has to go and look: a removal, a
     kind replaced, a batch, a subtree restored. Those are not what a
     drawing is built out of.
     1600 layers through a session: **584ms to 47**, and the walk that
     asked about copies was 85% of it.
     What was left after that was smaller, still superlinear, and named:
     `Document::parent_of` scanned every group's child list to find one
     layer's parent, so `ancestor_space` — what space is this layer drawn
     in — was a walk of the document wearing a different hat, and the
     dirty region asked for it twice a command. The document keeps a
     **parent map** now (`parents`, child to group, `#[serde(skip)]`):
     a file carries none, so a document read from one builds it on the
     first question, and the five places that change a child list —
     add, remove (and the subtree it detaches), restore, move — keep it
     in step. `parents_match_the_child_lists_after_every_command` holds
     them to it: after every command in the shared list and after its
     inverse, the map is the child lists read the other way, and *once
     built, exactly* — a stale entry for a layer that has gone is
     invisible to a check that walks only the live layers, and the first
     version of the test passed a removal that left one behind. It is
     not harmless either: it puts the count off, which reads as "never
     built", which rebuilds the map on every question — the walk this
     map exists to end, back again and silent. Held exactly, a move that
     forgets the map and a removal that forgets it are each caught.
     1600 layers through a session: **47ms to 19**, and four times the
     layers costs 4.4 times the work, where linear is four; it was nine.
     Both scaling tests take a ratio rather than a time, with a floor as
     well as a ceiling, so a slow or busy machine moves both numbers
     together, and both ceilings are six now — margin for a noisy
     machine and nothing else, where the session's was twelve with the
     gap named.
     And the guard needed a test that was not there. Making a copy's
     arrival go unnoticed broke *nothing* in the suite, which looked for
     a minute like the flag not mattering. It matters; the audit that
     covers it changes the original with a `SetKind`, which is one of
     the commands that still goes and looks, so a session that had
     missed the copy arriving was put right by the very next thing it
     was asked. A fade is not one of those, and it is nearer what a
     person does — so that is the test now, and it asks the dirty
     *region* as well as the pixels, since a pixel drawn into a buffer
     nobody reads is still wrong on screen. A move would not have done:
     a copy draws what the original draws with the original's own
     placement undone, which is what makes moving the original move only
     the original.
     The **render cache** answered the same way, and the answer is one
     page: exactly one surface after a render, after twenty-one renders,
     and after fifty rounds of edit-then-repaint. No growth, nothing
     leaked, the previous frame not kept beside the current one.
     Worth guarding rather than merely knowing, because of what a
     surface weighs. It is premultiplied linear f32, sixteen bytes a
     pixel: the 1200 by 900 page tested is 16.5 MiB and A4 at 300dpi is
     133. A second copy kept by accident is not a rounding error, and
     "keep the last frame to diff against" is a few lines away — made to
     do exactly that, the same run holds **1170 MiB** against a ceiling
     of 24.7 (`the_cache_holds_one_page_however_long_it_is_edited`).
     Its floor is the half-surface below: a cache dropped between calls
     would satisfy the ceiling for entirely the wrong reason, so the
     test asks that a frame is being kept before it asks that only one
     is.
     With something able to watch memory, the question the **unbounded
     undo history** invites could finally be answered rather than worried
     about. Nothing trims `undo`, so a long session grows for ever in
     principle. In practice a brush stroke costs about two thirds of a
     kilobyte: two thousand of them hold **1.4 MiB**, which is the
     artwork rather than the bookkeeping. A cap would be a product
     decision and not a fix, and the measurement is what says so.
     What the measurement *is* worth is as a guard on how undo is built.
     Undo here is an inverse command per step, so the history holds the
     edit and an edit's inverse is about the size of the edit. The other
     way to build undo, and the more common one, is a copy of the
     document per step — hard to tell apart by reading, trivial by
     weighing. Made to snapshot, the same two thousand strokes hold
     **1120 MiB**: eight hundred times the ceiling the test allows, so
     there is no reading of the number that mistakes one for the other
     (`an_edit_costs_about_what_the_edit_is`).
     Its floors matter as much as its ceiling, and both directions are
     asked: the strokes have to be on the layer, and holding them has to
     cost at least half their own weight, or the counter is not watching
     what it is supposed to. And undoing the lot has to move the inverses
     between the stacks rather than build new ones, which is the other
     way a history can quietly double.
     The same question asked of the other two things a file carries beside
     its manifest — a **font** and an **ICC profile** — found the third
     kind of answer: not a crash, not a wrong picture, but **memory that
     never comes back**.
     A registered face lives for the whole process. The shaper and the
     rasterizer both hold borrows of it, so its bytes are leaked on
     purpose to give them the `'static` they need. The leak came *first*,
     though, and the refusal after it: `Box::leak(bytes)` and then a `?`
     on the parse. So a `.chitra` carrying a face that will not parse
     handed over its bytes and never got them back, and the document
     opened cleanly with nothing to say so. A file can carry as many
     faces as it likes.
     Measured rather than argued, which took building something that
     could see it: a counting global allocator in a test binary of its
     own, since a global allocator is per-binary and nothing else in the
     workspace can watch memory at all. Twenty megabytes of not-a-font
     offered, **twenty megabytes still held**. With the check moved ahead
     of the leak, nought. That behaviour was unobservable until there was
     something built to observe it, which is the argument for the test
     rather than for a comment.
     The same instrument, turned on the registry once more, found the
     accident beside the design. Offering the **same face twice under the
     same name** — two documents carrying it, or somebody loading the file
     twice — parsed it afresh and kept it afresh each time, and the copy
     it displaced became unreachable: nine repeats of the bundled face
     held seven megabytes nothing could ever read again. Answered from
     what is already registered now, when the bytes match to the last
     one.
     Replacing a name with *different* bytes still keeps the old face for
     good, and that stays: the registry hands out a `&'static Fonts` and a
     render in flight may be holding one, so there is no moment it could
     be freed without a borrow outliving it. That is the design and the
     price of it — three quarters of a megabyte per genuine replacement —
     written down rather than left to be discovered by wondering where
     the memory went. The fix covers the case that is an accident and not
     the one that is a choice.
     The test is **one test and not two**, for a reason worth keeping: a
     global allocator belongs to the whole binary and cargo runs tests in
     a thread each, so split in two the halves measured one another — the
     second's seven megabytes landed inside the first's reading and
     failed it. That is a flaky test rather than a finding, and it took
     seeing a sabotage fail *both* halves to notice.
     The rest of what a file carries came back clean, and the negatives
     are worth their place: a face whose file is missing from the zip
     opens with the text in the bundled face; a face that is not a face
     opens the same way and is refused by name; and a profile that is not
     a profile is dropped with `set_cmyk_profile` saying so. All three
     were already decisions rather than accidents — it is only the bytes
     that were not being given back.
     The damage sweep has a reach it cannot extend, and past it sits a
     real crash. Every entry it writes is re-zipped with its own CRC, so
     damage to a PNG's *bytes* is refused one layer down — but **a valid
     PNG of the wrong size passes every check the container has**. A
     `.chitra` says a resource's size in its manifest and carries its
     pixels in a file beside it, and nothing in the format makes the two
     agree: another writer, a hand-edited zip, a merge gone wrong.
     They are held against each other once, in
     `Document::restore_resource_bytes`, and bytes that do not fit the
     declared size are dropped — the manifest is the source of truth, and
     a layer with no pixels behind it draws nothing. That one line had
     nothing asking about it end to end, and it is load-bearing: let the
     bytes through regardless and the resource becomes a four-by-two
     picture holding a hundred and sixty bytes, and the **SVG export
     panics inside the image encoder**. A crash while writing somebody's
     file, one line away.
     Pinned now end to end, both ways a picture can be wrong — a valid
     PNG of another size, and no PNG at all — through the open, the
     render and the export, with the unswapped file asserted to draw and
     to travel as a picture so that "draws nothing" cannot pass on a page
     that never drew. Refusing a whole document over one bad picture
     would be worse than losing the picture, so losing it is the
     decision; this is what says so.
     Everything else probed around it came back clean, and the negatives
     are worth as much as the find: the catch-all pattern that hid the
     gradient map exists nowhere else in the document model; the
     exporters omit an adjustment with a comment rather than silently;
     and an empty resource goes into an SVG as nothing rather than as an
     empty `<image>`, which a reader still parses.
     A different kind of question, after two rounds of the fixture strand
     returning nothing: **what does this do with a file that is
     damaged?** An editor opens files somebody else's disk wrote. There
     was an audit (`a_damaged_file_is_refused_not_survived`) and it had
     two soft spots. It damaged a document of *one rectangle* — a few
     hundred bytes of manifest, so flipping bytes in the first kilobyte
     mostly lands in the zip's own headers and never reaches the page —
     and it threw the loader's answer away, claiming only that nothing
     panicked. It is the fixture's file now, four kilobytes of masks,
     strokes, gradients, text, copies, regions, a palette and a ramp of
     colours, damaged right across rather than at its head: 421 damaged
     files against about 155, over a document that exercises the parser.
     The measurement itself is clean and worth having written down: of
     421, **397 are refused, 24 open, none panics, and all 24 are the
     file that was saved**. No silent corruption anywhere.
     The stronger assertion added beside it — refused, or identical,
     never quietly something else — **cannot be made to fail**, and that
     is recorded in the test rather than glossed. The zip's own CRC
     refuses damaged entry data one layer down, so the only flips that
     open are ones landing somewhere inert, and those cannot change the
     page. Two sabotages were tried; both were caught by the *older*
     assertions above it instead, not by the new one. By the rule from
     the discarded group invariant that would make it worthless, and it
     is kept only on a narrow argument: the guarantee lives in the zip
     layer, and recovery logic added *above* that layer — a repair
     feature, a lenient reader, a blank page handed back rather than an
     error — moves the property out from under the CRC without touching
     it. Insurance, not a live guard, and the test says which it is.
     Twice in that work a sabotage was read as passing when it had
     failed, because the check grepped for two particular strings rather
     than reading the result line. Third time this session that the
     instrument, not the thing, was what went wrong.
     The nineteenth was **a text block that wraps**, and it **holds**
     rather than earns — which is worth recording as plainly as the ones
     that found something. Every text block this document had held was
     one short word on one line, leaving the whole multi-line path
     unasked here: a line count, a line height, a second baseline, and in
     the SVG exporter a tspan of its own per line anchored at the
     alignment's x. Centred as well as wrapped, since alignment means
     nothing until a line is shorter than the block it sits in.
     Every audit held. So, the rule, the code it was meant to exercise
     was broken — twice, on the two things the shape newly asks. Make
     every line left-aligned in the renderer and two tests fail, both
     written for alignment. Make the *SVG exporter* ignore alignment and
     one fails, on the markup. So both halves were already pinned, and
     the shape adds reach rather than an assertion: a wrapped, centred,
     two-line block now travels through the file round trip, the
     clipboard, the undo runs, the dirty-region audit and both exporters,
     none of which had ever carried one.
     One thing the sabotage turned up that is worth keeping. The resvg
     witness draws the shared fixture and **can** see the text — drop it
     from the export entirely and the witness fails — but it cannot see
     the text *move* under an alignment change. So SVG alignment is
     pinned by the markup and not by the picture. That is the same shape
     of blind spot as the GPU fixture audit's, where a glyph has no
     interior to compare, and it is recorded rather than closed: the
     markup assertion does catch it, and a second witness for the same
     property would be belt over braces.
     The eighteenth was **an adjustment stated by a ramp of colours**, and
     it found a defect by the plainest route in this whole method: asking
     what the document had never held and then reading the code that
     should have handled it. Every adjustment here was an `Exposure` —
     one of thirteen kinds, and the one whose parameters are a single
     number — so an adjustment carrying a *list* had never gone through
     the file format, the clipboard or the undo runs, and an adjustment
     carrying a *colour* had never been walked by the palette at all.
     **A named colour in a gradient map never settled.** The walk that
     keeps a document's colours in step read
     `NodeKind::Adjustment(_) | NodeKind::Filter(_) => {}`, under a
     comment saying neither holds a colour of its own however much it
     changes them. Convincing, and wrong: a gradient map *is* a ramp of
     colours. So the palette moved, every layer followed, and that one
     stop kept what it was authored with — and went into the file that
     way. `Adjustment` and `Filter` have walks of their own now, each
     matching variant by variant, which is what makes the promise in that
     doc comment true: a kind holding a colour does not compile until it
     says so. The promise had stopped at the `NodeKind` and never reached
     inside it.
     One audit stopped holding on the shape alone — the file format's
     inventory, by the twelve fields a gradient stop cannot be missing,
     the same twelve a vector's own gradient wants.
     And then the rule the method carries: everything else held, so the
     code the shape was meant to exercise was broken, and **only the new
     test noticed**. So the shape was holding rather than earning, and
     what earns it is the audit written next: after the palette moves,
     nothing in the shared document still means what it used to. Asked
     *structurally* — the document is serialized and every `Named.means`
     in the JSON is read — because the obvious way to ask is to walk the
     colours, and the walk is the thing that was wrong. A colour the walk
     cannot see is one it cannot report either, so a test built on it
     would have passed on the very defect that prompted it. With that in,
     breaking the walk back fails on the shared fixture, which nothing
     else in the workspace does.
     The nineteenth was **a path with curves of its own and a hole
     through it** (`pierced`): every path in this document had been
     straight-sided or smoothed by rule with no handles written down,
     and none had a second ring, so the bezier flattening, the even-odd
     fill, and the arithmetic on handles had never been asked of by
     anything here. A lens rather than a blob, on purpose: two anchors
     on a line with handles long enough that the curves bulge well past
     them, so anything that took a path's extent from its anchors would
     be wrong about it by half its height. Every audit held, with the
     lens moved, faded and re-handled by the shared list — the bounds
     flatten before they measure — so the code the shape was meant to
     exercise was broken to see who would notice, and the answer was
     exact in both directions. Drop the second ring from the rings the
     GPU stencils, and both fixture audits fail
     (`every_layer_of_the_fixture_puts_down_what_the_cpu_puts_down`,
     `whatever_the_gpu_agrees_to_draw_it_draws_the_way_the_cpu_does`):
     the hole bites through the fixture. Make the flattening ignore
     handles everywhere, and *no* fixture audit fails — the CPU, the GPU
     and the bounds share the one flattening, so they agree with each
     other about the wrong curve — while both exporter witnesses, three
     anchor tests and the renderer's own bezier test fail on pages of
     their own. Worth knowing rather than fixing: the curve's
     correctness is held by tests that draw against another program,
     which is the only kind that can hold it, and the fixture's reach
     for this shape is the file round trip, the clipboard, the undo
     runs, the dirty region and the two renderers' agreement.
     The twentieth was **a curves adjustment** (`graded`), held to the
     pierced path as the ramp is held to its layer: a master curve and a
     curve per channel, four lists of points where every adjustment
     here had been one number or a list of colours, and one of the four
     left empty on purpose, the identity said by an empty list being a
     case of its own. One audit stopped holding, and it was the right
     one: the file format's inventory of fields a `.chitra` cannot be
     missing grew by one, the curves' `points` (the three channels
     carry `#[serde(default)]` and are not required, which is what a
     file written before channels existed needs). Everything else held,
     so the two renderers' tables were broken in turn. The CPU
     forgetting the red curve fails the table test and the whole-fixture
     audit. The GPU forgetting the *blue* curve failed nothing at first
     — the path the grade is held to is orange, with next to no blue,
     and a curve that pulled blue's highlights moved it by less than the
     audits' tolerance. That is the shape being placed rather than the
     audit being loose: lifted from the shadows instead, where orange
     has room to move, the same sabotage fails both fixture audits. A
     third try, making an empty channel list tabulate rather than stand
     for the identity, changed nothing, because an empty list already
     tabulates to the identity; a sabotage that cannot fail is not one,
     and it is recorded here so it is not tried twice.
     The twenty-first was **a ground that stands for a palette entry**:
     the second frame's ground is `lilac` by name, a second swatch
     beside `ink`. Every named colour in this document had been on a
     shape, in text or in a ramp, and a frame's ground is the one other
     place a colour lives, so the walk that keeps colours in step with
     the palette had never been asked about one. Every audit held, and
     then the walk was made to skip a frame's ground — and *nothing*
     failed, five hundred tests green. That was not the ground's fault
     but the palette audit's: it moved `ink` alone and asked what still
     meant ink's old colour, so a colour standing for an entry the test
     never touched was a colour it never asked about; a second name
     was invisible to it by construction. The audit moves every entry
     now, each to a colour none of them was, and asks that nothing
     still mean what any of it said — and with that, the same sabotage
     fails on exactly one colour, the lilac ground. Alongside: the file
     format's inventory grew by six (a named ground's name, its
     meaning, and the four numbers of that meaning) and by the curves'
     `points` the twentieth had earned and not yet written down; and
     the doc-scaling probe measures 800 and 3200 layers rather than 400
     and 1600, because at two milliseconds a full parallel run of the
     workspace — the GPU's tests among it — moved a reading past any
     ceiling that was not slack.
     The twenty-second was **text set along a guide** (`arc`): a short
     word on an open, smoothed arc, two pixels in from its start. Every
     text block here had sat in lines from its origin, so the other way
     of setting type — a guide flattened and its arc length tabulated,
     each glyph turned to the direction there, an offset along it, and
     a box taken from where the glyphs land rather than from the origin
     — had never gone through the file, the clipboard, the undo runs or
     the wire. Its two edits close the guide into a ring, which is the
     other branch of the walk (a closed guide wraps what an open one
     drops), slide the text along it, and fade it. One audit stopped
     holding, the file inventory, by the guide path's two required
     fields. Then the code was broken three ways. Glyphs left unturned:
     two tests fail, the renderer's own and the PDF's quarter-turn
     matrix, both written for the feature, and no fixture audit. The
     box taken from the origin for guided text: two more written for it
     fail, and again no fixture audit — and the reason is worth having.
     The box is at once the dirty region and the rectangle the raster
     is drawn through, and both renderers read the one raster; so a
     wrong box clips the glyphs to exactly the region that is
     repainted, the CPU and the GPU agree about the cut, and nothing
     over this document knows where the letters should have been. The
     picture of guided text is held by tests that know, and only by
     them. The guide kept out of the file altogether: four fixture
     audits fail — the file round trip, the stand-in file, the
     inventory and the wire — which is the shape's reach, measured.
     Not the clipboard, whose every-kind walk is a fixed list of one
     layer per kind rather than the document's layers, so a guided
     block had never been sent through it; it and the group round trip
     carry one now, and the clipboard's comparison of the kind written
     out is exact, so a guide that did not travel would be named.
     The twenty-third was **a levels adjustment** (`leveled`), held to
     the picture: an input black and white point, a gamma between them
     and an output pair — five numbers where every adjustment here had
     been one, a ramp, or the curves' four lists — right above the
     raster in the stack, since a run of held layers is held to the
     first unheld one beneath it, and over the one layer with tones
     enough for a black point and a gamma to be told apart on. Two
     audits stopped holding. The file inventory, by the five numbers a
     levels cannot be missing (and it turned out to be carrying the
     curves' `points` twice, which a set did not mind; once now). And
     `a_layer_that_cannot_be_seen_is_the_same_as_no_layer`, on the
     picture hidden — by an eighth of a channel somewhere else
     entirely. Not a defect but the audit's premise, narrower than it
     read: a layer that others are *held to* is not the same page gone
     as hidden, and neither is wrong. Hidden, the run held to it has
     nothing to be held to and draws nothing; deleted, the run falls to
     the layer beneath and grades that instead, which is what every
     editor does with a clipping group whose base is deleted. The
     audit's "gone" takes the held run with the layer now, which is the
     page an invisible layer was always being held against. Then the
     sabotages. The CPU forgetting the gamma fails three: its own
     levels test, the GPU's adjustment table, and the whole-fixture
     cross-renderer audit. The GPU forgetting the output black failed
     *only* its own table at first — at 0.05 the shift was a dozen
     levels on the picture's dark squares, inside the fixture audits'
     tolerance — so the black point was raised to 0.2, where the same
     sabotage fails both fixture audits. The twentieth's lesson again,
     and worth having twice: the shape is placed where a wrong answer
     has room to show, or it is merely held.
     The twenty-fourth was **a layer in soft light** (`soft`), a warm
     ellipse over the group, its edit swapping it for a colour dodge.
     Ten of the sixteen blend modes stood in this document and on the
     pages nobody wrote, and the six that did not were all separable —
     a dodge, a burn, the two lights, an exclusion — so a soft light's
     curve and a dodge's division had been drawn by nothing here; the
     five join the random pages' list too. One audit stopped holding,
     and it was a surprise worth having: the file inventory, by an
     ellipse's two radii — no vector ellipse had ever stood in this
     document as a *layer*, only as masks and the region. Then the
     sabotages. The CPU drawing soft light as hard light fails five:
     its own two blend tests, both fixture audits and the random pages.
     The GPU pointing soft light at hard light's slot fails four, the
     same less the CPU's own. Both renderers, both fixture audits, the
     random pages: earned rather than held, first try.
     The twenty-fifth was **a layer that stands at an angle**
     (`tilted`), a stroked rectangle turned thirty degrees and a fifth
     again as large, its edit turning it to forty-five with the two
     axes scaled apart. Every transform in this document had been a
     translation or an axis-aligned scale. Two *commands* turn things,
     so the undo and repaint runs had seen a rotation — but no layer
     had ever been saved, copied, exported or drawn by the second
     renderer while turned, and the pages nobody wrote place every
     layer by translation alone, so they had not either. Stroked as
     well as filled, since a stroke's reach is a length in the layer's
     own space and the box it needs on the page is that length carried
     through the turn.
     Every audit held, so the code was broken, and the first two
     sabotages said the same thing: the turned box read off one
     diagonal instead of four corners fails eleven tests, and a length
     carried through a turn read off the axes alone (`Transform::
     max_scale`) fails nine — both already well pinned, by tests
     written for rotation and by the undo invariants that carry a
     feather between spaces. What the shape is worth had to be
     *measured* rather than argued, the way the frame's was: the
     one-diagonal sabotage was run twice, once against this document
     and once against it with the tilted layer taken back out.
     `every_layer_of_the_fixture_puts_down_what_the_cpu_puts_down`
     **passes without the layer and fails with it** — the per-layer
     cross-renderer audit had no turned layer to be wrong about, and
     has one now. `whatever_the_gpu_agrees_to_draw_it_draws_the_way_
     the_cpu_does` fails either way, since the whole page is drawn
     together there and the commands' rotations reach it. So the shape
     earns one audit outright, and the honest account of the rest is
     that rotation was already held elsewhere.
     **Two defects about copies, found by asking the pages nobody wrote
     to turn things.** The twenty-fifth shape's entry ends by noting
     that those pages place every layer by translation alone. Teaching
     the generator to turn a third of them — an angle that is not a
     quarter of anything, a scale either side of one — broke three
     invariants at once, and the first thing each needed was the
     question *is this the turn, or a page that was always going to
     fail once the shapes moved*. Adding a draw to the random stream
     reshuffles every seed, so a new failure is not evidence of
     anything until that is asked. Both of the two below answered
     "not the turn": each reproduces with every layer upright, and
     each is written down as a page built by hand rather than a seed.
     The first: **a mask that hides nothing changed the page, two
     copies deep.** The cover a copy hands down — its mask and fade
     turned into a coverage — was applied by the branch that asks
     whether a layer *is* an adjustment, a filter or a clone. One copy
     deep that was enough, since the copy turns its own mask into the
     cover and the filter itself receives it; two deep, the cover was
     handed to another copy, which that branch did not recognise, and
     the work was done at full strength wherever it stood. The branch
     asks what the layer *draws* now, which is the third time this file
     has had to learn that and the first time it has been written into
     a test that says so
     (`a_mask_that_hides_nothing_is_no_mask_however_deep_the_copies_go`).
     The second is worse and was under it: **a copy of an adjustment
     held to the layer below vanished.** Being held to something is a
     coverage too, the same pass a mask on such a copy already takes;
     unrecognised, the copy went on a surface of its own, where what it
     copies was handed a transparent page to change and came back with
     nothing. Not wrong by a little — gone. The GPU backend lost it the
     same way, which is exactly why nothing had ever said so: the two
     renderers agreed, and the cross-renderer audits are the only thing
     that would have looked. It is fixed in the reference renderer and
     declined in the backend, each with a test of its own, and the
     lesson is that two renderers agreeing is not two witnesses when
     they share the mistake.
     A third defect came out of the same run, in the backend: **a held
     layer's effects escaped what held it.** A layer's own mask and what
     it is held to ride the one coverage slot there, and both were
     folded into the layer's drawing and then left off the pass that
     lays the surface down. For a mask that is right, and deliberately:
     the mask shapes what the effects grow from, and a shadow of a
     masked shape falls outside the mask. For a clip it is wrong — a
     clip cuts what the layer lays down, its effects with it — so a
     held layer drew its outline outside the layer it was held to,
     where the reference renderer draws none. The lay-down keeps a
     coverage of its own now, built from what holds the layer back and
     not from its mask
     (`a_held_layer_keeps_its_effects_inside_what_holds_it`). The first
     guess was wrong and is worth recording: the coverage texture is
     sized to the layer's own box, so an effect standing outside it
     looked like a fragment reading past the texture's edge. Growing
     the texture by the effects' reach changed nothing, because the
     effects were not reading that texture at all.
     The repainted rectangle's seam was **two more defects in the
     reference renderer**, neither about turning, and both found by
     minimising the page until one layer was left.
     The first: **a stroke reaching into the rectangle was not drawn
     where the shape it belongs to did not reach it.** A shape's box was
     cut to the region being painted *before* the stroke's reach was
     added to it, so a rectangle that missed the box left nothing for
     the reach to grow. The box is grown while it is still a box of
     numbers now, before it becomes pixels at all — which matters
     because the case that bites is a layer on a surface of its own,
     where the shape lies off that surface entirely and has no pixel box
     to grow. Drawn straight onto the page the old arithmetic happened
     to come out right, which is why the test for it carries a blend
     (`a_stroke_reaching_into_the_region_is_drawn_there`).
     The second: **a copy of a layer whose mask is feathered.** Every
     surface is given room for the softenings that will land on it, and
     a copy was asked for its *own* mask's margin — but a copy usually
     carries no mask at all, since what it carries is what it copies.
     The surface came out exactly the size of the rectangle and the
     feather clamped at its edge. `softening_within` walks what a copy
     copies and what a group holds, in the space the drawing happens in,
     which is why a copy that shrinks what it copies showed this and one
     that magnifies it did not
     (`a_copy_of_a_softened_mask_repaints_to_the_whole_page`).
     A wrong first cut of that one is worth recording: growing the
     layer's *extent* by the same margin fixed the seam and broke
     `every_command_repaints_every_pixel_it_changes`, because a
     softening inside cannot make a layer land further out, and drawing
     it there put ink outside the region the engine had called dirty.
     The surface and what is laid down are two rectangles now, and only
     the first is grown.
     The last of it was **an effect on a layer held to a turned one**,
     and it took four tries to see because every reconstruction of the
     failing page came out clean. What finally separated it was asking
     the two renderers to *draw the same simple page and printing both*
     — a base, a layer held to it hanging off its side, an outline on
     that layer — at which point the picture said it in one line: the
     outline traces the **cut**, and where the cut runs at an angle it
     runs with it. Both renderers cut the layer before growing the
     effect, and both cut again afterwards; the second cut changes
     nothing where a coverage is nought or one, and where the cut is
     diagonal it is neither, so the two compound a soft coverage
     differently — the outline along the cut comes out two pixels wide
     in the reference renderer and one in the backend, fading where the
     coverage falls. Which is right is a question about what a soft
     clip *means* rather than a defect with an answer, so the backend
     declines that one combination (an effect on a layer held to
     something not upright) and draws the upright case, where the two
     agree exactly. `a_held_layer_wearing_an_effect_goes_back` pins the
     decline and writes down what the reference renderer does, so
     whoever settles it finds the evidence rather than the conclusion.
     **Settled (October 2026), and it was a defect after all.** The
     reference renderer did *not* cut again afterwards: it wrote the
     effect inside the window the base had been drawn in, and that window
     is grown by how far the held layers' effects reach — so they can
     *read* past the base — which let them *write* past it too. On a
     turned base the window is bigger than the base, and the outline
     "tracing the cut" was the rectangle showing; on a disc, a held
     layer's shadow filled the corners of the disc's box on the page. It
     cuts by the base's alpha now (`stamp`'s `held`), as the backend
     does, and the turned case is drawn by both, agreeing to a
     two-thousandth (`a_held_layer_wearing_an_effect_on_a_turned_base_is_
     drawn`).
     Two false starts are recorded with it, both plausible and both
     wrong: that the coverage texture was too small for the effect to
     read (growing it changed nothing — the effects were not reading
     that texture), and that the coverage was being applied twice
     (taking one of the two away changed nothing either, and made the
     backend stop tracing the cut at all, which is how the real
     mechanism came into view).
     **The pages nobody wrote turn a third of their layers now.** What
     that cost, measured: the coarseness watch rose from 27% of pages
     with a rough pixel to 27% at a worst of 0.726 against a ceiling of
     0.70, and is re-based to 30% and 0.75. An edge at an angle is the
     one an edge-based rasterizer and a sampled one disagree about
     most, and before this every edge on every one of these pages was
     vertical or horizontal — so the rise is the measure doing its job.
     The interiors, which are the claim, did not move at all.
     **And they hold clone layers now**, half of whose strokes heal —
     the one kind no page nobody wrote had held, so no clone had ever
     met a random mask, blend, turn, copy or effect. Adding a kind
     reshuffles every seed, so each finding was asked whether it was the
     clone or the reshuffle, and both kinds turned up. Five came from
     the clones:
     - **A crash.** A clone layer held to the one below and wearing an
       effect read the holding coverage over a region grown by the
       effect's reach, past the window that coverage spans: an overflow
       in a debug build, an index out of bounds in a release one — the
       engine stopping, in the browser. Three audits over these pages
       and the file round trip panicked on it. Its region is now cut to
       that window first, as every other held layer's surface is, and
       that also stopped its shadow falling outside what holds it
       (`a_held_clone_wearing_an_effect_shows_only_where_it_is_held`).
     - **A copy of a clone vanished** whenever it went to a surface of
       its own, which has nothing under it to lift from: faded or masked
       (seed 426, found by the blend-coverage audit, since with a blend
       on the clone the copy took another road), wearing its own blend
       (seed 1263, the next page that audit stopped on), masked while
       blended (seed 514, the mask-identity audit), or held to the layer
       below. One road now serves every copy of a clone: drawn where it
       stands, its own blend taking the clone's place on each stroke,
       then mixed back by its mask and fade — exact at every coverage
       for one composited picture, and the copy unmasked at coverage one
       (`a_copy_of_a_clone_lifts_where_it_stands`; its held case first
       read one channel and let the sabotage through, blue and red
       sharing a green of nought). A copy wearing an effect of its own
       vanished too, and was the one left for a second sitting: it wants
       a silhouette, and a clone's is what its strokes lay, drawn aside
       as they land — which the clone's own branch already builds. So
       the copy is resolved down to the clone it copies
       (`clone_behind`): the clone's strokes in the space the copy puts
       them, the clone's mask and fade on each stroke, every copy's on
       the way down letting through part of what was laid, the
       outermost blend that is not Normal bringing it down, effects and
       all. Held against a twin — a second clone layer with the same
       stroke where the copy puts it, the same shadow — to a
       ten-thousandth, masked and unmasked; each half fails when its
       step is taken out. What is still not resolved, and still draws
       nothing, is a chain with an effect *inside* it — on the clone
       itself or on a copy between — since the picture the outer copy
       then copies is the inner one's shadow as well as its strokes.
       The GPU backend draws a plain copy of a clone and hands every
       dressed one back.
     - **And two roads out of the page that assumed a clone has a
       picture of its own.** PDF export placed a clone layer with the
       run of layers that go over as pixels, rendered with only that run
       shown — nothing under it to lift, so every PDF of a retouched
       page came out without the retouching, while the file read
       perfectly well. It now flattens what is under it, as an
       adjustment and a filter always did, and so does a copy of any of
       the three, which the old kind check missed as well. A *group*
       holding one does not: the engine isolates such a group, so the
       group drawn alone was already right — which the first draft of the
       test did not know, and its group case passed with the rule taken
       out; a copy of the clone is what reaches through, and is what the
       test asks now (`a_clone_layer_goes_over_as_what_it_lays`, read
       back by Ghostscript). And a layer's thumbnail, like the view that
       shows one layer alone, draws it on nothing: a clone's was a
       transparent square in the layers panel. A clone at the top of the
       page, or a copy of one there, is now shown as what it lays, lifted
       from the page below it drawn aside over the region asked for
       padded by the document's reach — which counts the clone's own
       offsets and a heal's length, so a source off the thumbnail's square
       or off the part of the page on screen is drawn too
       (`a_clone_layer_on_its_own_shows_what_it_lays`, and block 9i in the
       browser). An explicit union of every stroke's source was written
       first and then taken out: the reach already covered it, and a
       sabotage of the union could not show. Inside a group it now works
       too, one sitting later: a group holding a clone is always isolated
       (`reads_backdrop`), so what its clone lifts is the group's own
       earlier layers and nothing outside it, and that is drawn aside as
       the document with the group's ancestors made plain — no fade,
       blend, mask, effect or hold — everything off the path down to it
       hidden, and its own layers from the clone on hidden. The test
       gives the group a mask that hides the patch the clone lifts (a
       group's mask is over the finished group, so the page still lifts
       it), a layer above the clone lying over its source, and an offset
       big enough to move the dab off the patch; four sabotages — siblings
       left showing, ancestors left dressed, later layers left showing,
       the page's space taken for the group's — each fail it, and the
       last two passed until the layer above and the larger offset went
       in. A *frame* above the group is made plain the same way, drawn
       aside without its ground — which is behind the group rather than
       in it, so the page's clone never lifts it — and a frame case with
       a green ground fails the test when the ground is left in. And SVG, which wrote a clone layer as a comment saying it
       had no equivalent, now carries what it lays as an image the way it
       carries a brush layer's paint (`clone_pixels`, the same drawing
       aside, bare so the SVG wrapper's own opacity, mask and blend are
       not taken twice — which fades overlapping strokes as one where the
       page fades each as it lands); read back by resvg with the layer
       placed through a transform of its own
       (`a_clone_layer_travels_as_what_it_lays`). A *copy* of a clone
       keeps the comment: the original's markup again would carry what
       the original lifted to the copy's place. The last of these roads
       was **picking out what a layer covers** (`pick_from_layer`, and the
       subject picker that asks the same question first): it draws the
       layer alone and traces what it covers, and a clone alone covers
       nothing, so every one answered "that layer covers nothing on the
       page". What a layer *occupies* is asked, not the picture it makes,
       and a clone occupies its strokes — the geometry a click on the
       canvas already picks it by — so for that question each clone the
       layer is, copies or holds is drawn as a brush laying the same
       strokes in solid paint. Asked of a clone lifting from bare page,
       which lays nothing visible at all, and of a copy of it moved
       elsewhere (`a_layer_says_what_it_covers`); taking the conversion
       out, or not following copies, brings the old error back.
     Two came from the reshuffle and are older than the clones:
     - **A surface cut before it was grown** (seed 259): a copy — and,
       the same code again, a group — is given room for the feathered
       masks inside it to soften over, and the room was added to a box
       already cut to the layer's own, so wherever the box ended first
       the softening met a wall. A copy of a feathered layer moved three
       pixels when given a mask that hides nothing
       (`a_feathered_mask_softens_the_same_on_any_surface`, one half per
       road, each failing alone when its fix is taken out).
     - **A copied blend spent on a surface** (seed 294) — the long-known
       limit written up below, in the one case `copies_a_blend` leaves
       out: a blended layer wearing effects, which a `Cover` would cut
       before they grew. Drawn where it stands and mixed back instead;
       a mask that hides nothing had moved seventy pixels by a fifth.
     The GPU's watch went from 1491 pages drawn to 1377 — the new
     declines are effects on clone layers, dressed copies of clones,
     and the held-layer rules meeting clones and groups-with-effects as
     bases, all on the backend's hand-back list — and its worst pixel
     from 0.726 to 0.922, on a page with no clone on it (seed 1178): an
     outline measured from a yes-or-no silhouette, one pixel of a
     turned path's edge at 0.55 alpha from four samples against 0.43
     exact, in on one side and out on the other. One page can own a
     maximum that way, so the watch now counts the pages at or past
     0.75 and names them (one), rather than holding the maximum alone.
     **And every adjustment, not three.** The pages held exposure,
     brightness/contrast and hue/saturation, and the fixture five kinds,
     so white balance, vibrance, black and white, invert, selective HSL,
     shadows/highlights and colour balance had never met a random mask,
     blend, turn, copy or held layer. Everything held — the GPU drew 1474
     pages against 1477 — so the method's other half: ten sabotages in
     the backend's adjustment code, one per kind, each run against the
     suite with the widened pages and without them. Three had been
     noticed by **nothing at all**: black and white left unnormalized,
     invert dropping what lies above white, and selective HSL giving a
     pale colour the whole of a band's change. The widened pages caught
     all three, but only the statistical audit did, so each got a
     pointed ask in the dedicated tests, which is where the reason for
     the gap was: the black-and-white weights there were 0.5, 0.3 and
     0.2, which already add to one; no page had anything above white
     for an invert to carry through (an exposure under it now puts the
     light strip there); and the bands' strips were all saturated enough
     to take a band's whole change, and warm where the band set was the
     blues (a pale blue strip now). The same instrument that found them
     caught nothing new on the reference side — the widening's value is
     the three holes, each now closed by a test that names it.
     **And every kind of mask, and a stroke dressed every way.** Masks
     here had all been a vector ellipse; now they are any vector shape, a
     brushed one or a picture's brightness. Strokes had all been plain;
     now they are dashed, capped and joined each way, kept inside or
     outside, marked at their ends and swelling. And a gradient can be
     radial. Four defects, three of them the reference renderer's:
     - **A repainted rectangle left a seam over a feathered brushed
       mask** (seed 500). Every kind of mask worked its softened plane
       out over the region grown by what the blur reaches — except a
       brushed one, which returned early with a plane cut to the region
       and blurred there, fading out along the region's border
       (`a_brushed_mask_softened_repaints_without_a_seam`).
     - **A blended copy of a group changed what the page covered**
       (seed 1570). The blend sends the copy to a surface of its own,
       grown only by the copy's own feather — none — so a feathered
       mask inside the group softened against the surface's edge. The
       growth tried and taken back two sittings earlier, for want of a
       page that needed it, was exactly the fix; this was the page. The
       feathered-mask test has a third road now.
     - **A feathered picture mask was empty in its thumbnail and hid
       everything in an SVG** (found reading the code while chasing the
       second). Both worked the softened plane out with no document, and
       without one a picture mask has no picture: it read as hiding
       everything, blurred. The page, which passes the document, was
       right. The document-less helper is gone
       (`a_feathered_image_mask_is_what_it_lets_through_everywhere`).
     - **The GPU drew a dashed rectangle or ellipse solid** (seed 1116):
       their strokes are a band from the signed distance, which knows
       nothing of a pattern, and every dash in the fixture was on a path,
       which goes through the reference's pieces. A dashed one now goes
       that way too (`a_dashed_rectangle_and_ellipse_are_dashed_here_too`,
       read as stroke and gap on both renderers).
     The GPU drew 1478 of the pages after; every other audit held.
     **And frames that hold things, text set every way, brushes that
     erase.** Frames here had been empty or absent; now a page can hold
     one, with or without a ground, and frames get children as groups
     do. Text had been one plain line; now it is underlined, struck,
     aligned, spaced, set in a width, given a run in its own colour and
     set along a curve. A brush layer can carry a second stroke that
     erases or is held to a region. Seven defects, three the reference's:
     - **An upright frame's edge moved when it was faded, blended or
       masked** (seed 3): plain, it is cut to whole pixels; on a surface
       it was cut by coverage, the turned frame's treatment, so a frame
       at half a pixel lost half its last row under a mask that hides
       nothing (`an_upright_frame_keeps_its_edge_however_it_is_composited`).
     - **A faded or masked frame took away what an adjustment inside it
       did** (seed 152): a plain upright frame is only a narrower region
       to paint in, so its contents see the page; on a surface they saw
       nothing. It is drawn in place and mixed back by mask and opacity
       (`a_frame_holding_an_adjustment_works_on_the_page_however_it_is_faded`).
     - **Blended, copied or given an effect, the same frame lost the
       page again** (seeds 4 and 51): a turned frame holding a heal,
       copied upright, lifted the page at Normal and nothing at Hard
       Light; a masked copy and an invisible shadow did the same. The
       blend now meets what the frame *paints* (from a surface of its
       own) and what its contents made of the page is added as it was;
       effects grow from that painted silhouette
       (`…_blended_or_copied`, `…_under_an_effect`).
     - The GPU drew a text run in its own colour in the block's
       (seed 112): a tinted block now carries an ink texture.
     - The GPU faded a leaf's fill and stroke once for both, where the
       reference fades each as it paints (seed 611).
     - The GPU put a clipped stroke's region, a mask and an effect in one
       texture slot (seed 1195), and drew a masked copy of an effected,
       blended layer and an effect held to a brush layer its own way
       (seeds 1104, 223): the first is kept apart, the other two go back.
     The GPU draws 980 of the 2000 pages now, fewer because frames that
     hold things and text along a curve go back; every audit holds.
     **And copies that differ from what they follow.** Every copy on
     these pages drew its original entire; a copy with a layer of its own
     in place of one of the original's had met a mask, a blend, a turn or
     an effect only on the page somebody wrote. Half the copies of a
     plain group now take a stand-in (moved, refilled, sometimes blended;
     a group stood in for by a copy of it, as `override_child` does), now
     and then with a layer of their own standing in for nothing, and a
     quarter of the pages end on a plain group and a copy of it dressed
     like anything else — left to chance, fewer than one page in a
     hundred had one; 279 of 2000 do now. A third of the pictures are
     see-through in places, where every one had been opaque. The
     widening found nothing wrong with stand-ins, and that is now worth
     something: drawing a masked copy's original instead of its
     stand-ins, or no longer isolating stand-ins that read the page,
     failed **no** renderer test before this and fails three audits
     after. One defect, found by the pages' new random stream:
     - **A copy of a clone layer wearing a shadow cast none** (seed
       1343). A copy draws what it copies, effects and all, but a copy
       of a clone goes its own road — drawn where it stands, to have
       something to lift — and that road drew the strokes alone. The GPU
       drew the shadow, and disagreed. `clone_behind` now takes the
       effects from whichever one layer on the way down wears them, and
       a plain copy over an effected clone is drawn as effected
       (`a_copy_of_a_clone_layer_casts_the_shadow_it_copies`).
     The GPU draws 858 of the pages; every audit holds.
     **And what groups and frames hold, dressed.** A layer inside a
     group or a frame had only ever been placed: no mask, fade, blend,
     turn, effect or clip on any of them, two levels deep, which is where
     isolation decides what a blend or an adjustment sees. Half of them
     are dressed now like a layer on the page. And the repaint audit asks
     pages with effects too — they were left out on the argument that the
     engine grows the region, but it grows it for the layer that changed,
     and a layer wearing a shadow elsewhere can straddle its edge (382
     pages compared now, from about 300). Six defects:
     - **A frame drawn in place let a soft-edged layer out past its
       edge** (seed 1093): a group under a feathered mask lays its soft
       edge a little past the region it is given, and in place that
       region is the frame's. What the contents lay outside is put back
       (`held_inside`; `a_frame_drawn_in_place_keeps_a_soft_edged_layer_inside_it`).
     - **A frame masked away showed whole once a layer in it took a
       blend** (seed 361): the in-place road read a feathered or brushed
       mask with no plane worked out, which lets everything through —
       a defect of the road added last chunk
       (`a_frame_masked_away_hides_a_layer_that_reads_the_page`).
     - **A shadow cast into a frame from past its edge was cut short**
       on the frame's own surface, whose bounds are its box
       (`a_shadow_cast_into_a_frame_from_past_its_edge_is_whole_when_faded`).
     - **Surfaces cut to a repainted rectangle cut a shadow's silhouette
       or a feathered mask's neighbours**: the generic effects road grew
       its extent by what is inside but not its pad around the region;
       a layer held to another asked the base's alpha only as far as its
       own feather reached; the group and copy arms knew softening but
       not shadows (seeds 980, 24). `effects_within` beside
       `softening_within`, used wherever a surface is sized.
     - **A blended frame's side surface drew more than the frame**, so a
       pixelate inside averaged its edge blocks over more (seed 51): it
       is drawn over the frame's region, with the room only around it.
     - **A shadow blurred by a sixth of a pixel reached three pixels
       where `Effect::reach` said half of one** (seed 172): each box of
       the blur is held at a radius of one.
     The GPU draws 858 of the pages; every audit holds. Taken out
     again one at a time, every one of these fails an audit or its own
     test, bar one: drawing a turned frame's contents over the laid
     region only, which keeps what a pixelate there averages exactly
     what it was before the surface grew.
     **And a reader that is not us, on the pages nobody wrote.** The
     exporters had been held against another reader on one page somebody
     wrote. `a_reader_covers_what_the_engine_covers_on_pages_nobody_wrote`
     exports 800 random pages to SVG, has resvg draw them and compares
     *coverage* with the engine — which composites the same in linear
     light and in the shown encoding, so it has one answer even where
     colour legitimately differs (the partial-alpha divergence above).
     Text, outlines and filters that do not travel are left out of both
     sides, being approximate there by design. Half the pages disagreed
     the first time; ten now differ by a few pixels of antialiasing or a
     turned picture resampled. What it found:
     - **Every effect was left out of every SVG.** Shadows, outlines and
       inner shadows now travel as SVG filters on a wrapper outside the
       layer's transform, mask and hold, carrying its blend (a filter
       isolates what is inside it). The outline is grown by a box in SVG
       and by a true distance here, so it is square at a turned corner.
     - **Every smooth path went out as a polygon** through its anchors,
       with a note no reader reads; it goes out as the cubic its
       Catmull-Rom curve is (`smooth_handles`).
     - **A self-crossing path was filled where the page has a hole**:
       even-odd was written only for paths with extra rings. Every path
       is even-odd now, as here and in the PDF (`f*`).
     - **An arrowhead on a closed shape**, which the engine does not draw.
     - **Holds.** A layer held to a hidden layer, an empty group, or a
       copy of one travelled unconfined; one held to a clone layer was
       confined to nothing (the base drawn aside lifted nothing); a copy
       of a held layer carried the original's hold to its own place.
     - **A group holding an adjustment lost its mask** — its box is
       "everywhere", and the mask was only worked out over a box.
       `parent_box` works it out over the page.
     - **An upright frame was cut at its exact box** where the page rounds
       it to whole pixels, and a copy of one was rounded where the
       original stands; the writer now carries each writing's placement.
     - **Every exported picture off the page's pixel grid was smeared** by
       a reader resampling it — brush layers, clone layers, masks, holds.
       They start on whole page pixels when their placement is a move
       (`on_the_grid`).
     - **A copy of a clone layer was left out**; it travels as what it
       lays, as the clone does.
     And three in the renderer itself: **what is held to a clone layer**
     was let through everywhere, as over an adjustment, though a clone has
     a shape — its effects already grow from it; **what is held to a copy
     of an adjustment** vanished, the copy having been drawn aside to be
     cut by and covering nothing (`a_layer_held_to_a_clone_layer_shows_where_it_laid_something`,
     `a_layer_held_to_a_copy_of_an_adjustment_shows_whole`); and **a
     turned view** cut the page at the box round it, so artwork past the
     page's edge showed in the corners (`a_turned_view_shows_nothing_past_the_page_edge`).
     Taking the pages apart to find these found one more:
     **a page that would not draw** after a layer inside a symbol was
     removed, where the copy's stand-in for it was itself a copy — one
     question asked of it went looking with `?`. Every layer of 400 pages
     is now taken away in turn, and each page must still draw and export
     (`taking_any_layer_away_leaves_a_page_that_draws_and_exports`).
     Each fix, taken out again, fails an audit or a test; the hold to a
     clone needed a test of its own reading colour, since the clone
     covers the same pixels either way.
     **And ghostscript, for the PDF.** The same question of the PDF
     exporter, over 300 pages
     (`ghostscript_covers_what_the_engine_covers_on_pages_nobody_wrote`):
     one run of ghostscript renders every page's PDF onto transparency,
     images interpolated — a placed picture is rendered at up to four
     times for print, and without interpolation ghostscript takes one
     sample in sixteen and aliases every edge — against the engine
     rendered at those four times and averaged, since at that resolution
     a frame's edge rounds to a quarter pixel and that is the picture the
     PDF carries. Text is left out: ghostscript sets it lighter and half a
     pixel right of both the engine and resvg, which agree to a tenth.
     Two thirds of the pages disagreed the first time. Found:
     - **A copy of a hidden layer was in every PDF** and on no page.
     - **A copy pointing at a removed layer stopped the export** — and,
       once it did not, left its placement on everything drawn after it,
       as **a copy with stand-ins always had**: an early return past the
       `Q` that closes its state. The copy's arm is a block left by
       `break` now, and `every_state_a_page_saves_it_restores` reads the
       stream itself, since the pages put their copies last where a leak
       has nothing to land on. The removal audit now exports a PDF too
       wherever a removal leaves a copy pointing at nothing.
     - **A layer held to a filter was in no PDF**: the filter went into a
       picture of everything under it, and the held layer into one of its
       own with the filter hidden, where the hold let nothing through. A
       layer and what is held to it now always go down together.
     - **A copy with a blend drew nothing**: its picture was made with
       every other layer hidden, its original included. The others are
       put aside into a hidden group instead, where a copy still finds
       its original visible.
     - **A copy faded to a third was drawn solid** — live, with nothing
       applying its opacity. A faded or blended copy goes as pixels, as a
       faded group does.
     - **Every placed picture started at a quarter of a pixel**, cropped
       at its ink; it is widened to whole page pixels.
     - **A regular stretch after a bold one was stroked**: the text render
       mode is graphics state and outlives `ET`
       (`a_regular_stretch_after_a_bold_one_is_not_stroked`).
     Left open, measured (and since closed, below): a synthesized bold
     in a PDF is the outline stroked, which grows every edge both ways,
     where the engine smears the outline sideways — ghostscript puts on
     twice the ink the engine does. A stroke under a space squashed flat grows sideways only, and
     was written, but ghostscript draws a sub-pixel stroke as a hairline
     whatever its shape, so nothing here could say whether it was right;
     it was taken back out. And an upright frame drawn live in a PDF keeps
     its exact box: rounding it to whole 72-dpi pixels, as the SVG does,
     is a screen's rule, and the audit could not tell the two apart.
     **And back in again: an exported page, brought back, covers what it
     covered.** `a_page_exported_and_brought_back_covers_what_it_covered`
     exports 400 random pages to SVG, places each back the way
     `Session::place_svg` does and asks the engine to draw both. The
     first time, 110 pages disagreed, all for one reason and one more:
     - **A clip or a mask on a placed group came in at the page's
       origin.** usvg keeps a clip's and a mask's contents in the space
       of the group that refers to them, not placed; the region is now
       carried through the group's transform (and the clip path's own)
       (`a_clip_on_a_placed_group_lands_where_the_group_is`).
     - **Text in a placed group came in at the page's origin** for the
       same reason: flattened glyph outlines carry none of the text's
       placement (`text_in_a_placed_group_comes_in_where_it_stands`).
     **And text, which the reader audit had left out.** Once it was put
     in, every piece of it that disagreed was ours:
     - **A newline between two tspans was a space** under
       `xml:space="preserve"`, so every right- or centre-set line ended a
       space further left in a reader. Nothing is written between them.
     - **Text on a guide slid back along it** by its `text-anchor`, which
       a reader applies again on top of the offset already worked out.
     - **A guide in the root `defs` was read in the root's space** by
       some readers and the text's by others; it is written beside the
       text, inside the text's own group.
     - **Underline and strike-through were left to the reader**, which
       puts them where its own font tables say; they go out as the
       engine's own bands, one rectangle each.
     - **The engine smeared every block placed off the pixel grid.** A
       block was rasterized on a grid of its own and read off it between
       texels, so half a pixel's nudge drew every stem across two pixels
       at half strength — on screen, not only in the audit. The raster is
       now laid on the grid the block is seen on (`grid_phase`, for text
       upright, turned a quarter or mirrored), and the GPU, which reads
       the same raster through its `origin`, follows. Its underline,
       rounded to whole texels, lay up to half a row from where every
       exporter puts it; bands now cover what their rectangles cover, the
       pieces a run boundary cuts one into adding up at the seam
       (`text_off_the_pixel_grid_is_as_sharp_as_text_on_it`).
     With text in, the reader audit touches fewer pages than it did
     without it — fourteen, none by more than five pixels. Text on a
     closed guide stays out (SVG cannot wrap past the guide's start), as
     does synthesized italic and bold, which a reader does not
     synthesize. Each fix, taken out again, fails an audit or its test.
     **And text in the PDF audit.** It had been left out because
     ghostscript "sets it lighter and half a pixel right". Neither was
     the PDF. Ghostscript's type at the page's own 72 dpi is a quarter
     lighter than the same file at 288 or 1152 — its rasterizer at small
     sizes — so ghostscript is now read at four times and averaged, as
     the engine's side already was. What was left was ours:
     - **An underline thinned as the view zoomed in.** Its floor of one
       pixel was a pixel of the raster, so at 400% a 10 px block's
       underline was 0.45 page pixels thick where the exporters write 1
       (`an_underline_is_as_thick_at_every_zoom`).
     - **A synthesized bold was a fifth heavier in every PDF** — the
       "left open" above. It was stroked (`2 Tr`) with a round pen that
       grows the outline up and down as much as sideways; squashing the
       pen flat does not survive a reader, which strokes text under the
       text matrix and undoes the squash (ghostscript does, measured).
       The ink is now drawn as the page draws it: the outline swept
       along the baseline is exactly the outline plus the band each of
       its edges sweeps — a curve cut where it turns back across the
       sweep, so each band runs one way (`one_way_across`) — one path
       wound one way and filled once (`heavy_run`), with the text set
       invisibly (`3 Tr`) over it so it is still found and copied. One
       paint also fades a faded bold once, where fill and stroke
       overlapped and faded twice
       (`a_synthesized_bold_puts_on_the_ink_the_page_does`). Outlines are
       text-heavy, so page content is now compressed: a bold sentence's
       PDF went from 111 KB to 39 KB, and a plain one shrank too.
     With text in — bold, italic, on guides — none of the 300 pages
     differs anywhere by more than half a pixel's coverage.
     **And two more readers.** MuPDF, installed for the purpose, read
     the same 300 PDFs at four times and agreed with the engine on every
     page but one pixel — two independent readers now say the PDF is
     what the page is, so it was not made a standing test. Chromium read
     the 800 SVGs (Playwright, the repository's own DejaVu pinned by
     `@font-face`) and found one thing that was ours: **a browser counts
     letter-spacing after a line's last letter**, so every right-set line
     with spacing sat one space left of the page's and every centred one
     half a space, where resvg, which does not, agreed. No line is left
     to `text-anchor` any more: each starts at the x the engine's layout
     starts it at, so a reader has only to agree on advances. Split so
     that neither side's resolution is what is measured — everything
     but text at 1× (at 4× a browser upsamples the brush strokes and
     masks an SVG carries as pictures, which the engine draws from
     geometry), text at 4× (at 1× Skia's text contrast thickens every
     stem) — Chromium differs on 11 and 15 of 800 pages by at most a few
     pixels. What it still does its own way, measured and left: a frame
     is rounded to the page's pixels in the SVG and to the view's in the
     engine, and a shadow on text along a guide it blurs wider.
     And a correction to the chunk before: compressing the PDF's pages
     broke two of the browser suite's checks, which read operators off
     the file's raw bytes, and the run that called it green was against
     an engine build that had not been redone — its build was piped
     through `tail`, which hid the exit status. The suite reads a PDF's
     drawing inflated now (`pdfDrawing`), and a build is checked by its
     own exit code before the suite is believed.
     **A brush stroke was thinner than its size, by the zoom.** Looking
     at what an SVG would need to carry a brush stroke as vectors started
     with the stroke's exact edge, and the edge was not where the stroke
     is: a hard stroke's antialiasing ramp, one device pixel wide, sat
     wholly inside its radius, so the stroke was drawn half a device
     pixel thinner on each side than it is — and since that half pixel is
     the view's, not the page's, how thin depended on the zoom. A stroke
     two across laid 115 pixels of ink at 100% against 157 of geometry,
     146 at 400% and 151 at 800%: the same stroke visibly thickened as
     the view zoomed in, and was a quarter lighter on the page than in
     anything that drew it from its shape. The ramp is centred on the
     edge now (`brush_edge`, and `fs_brush` beside it), so the ink is the
     geometry at every zoom (156.9, 156.7, 156.6, 156.6); a soft stroke
     keeps its fade inside the radius, widened by the same half pixel
     each side. The GPU's segment quads grew by a band, since a quad
     draws only the pixels it covers and the edge now reaches past `r`;
     the CPU's boxes needed nothing, rounding outward already takes in
     exactly the pixels whose centres are within half a pixel of the
     edge. One test had been reading the old thinning — an edge falling
     on a pixel boundary has no partly covered pixel to call a rim — and
     now draws its stroke off the grid.
     And one audit stopped holding, which was the finding that mattered:
     **a blended clone layer blended each stroke as it landed**, so where
     two strokes overlapped the second was screened against the first as
     well as against the page, while the same layer drawn aside — as a
     shadow at no opacity sends it — blended once. The moved edge put an
     overlap onto a page the no-op-effect audit reads. Blend-once is what
     every other kind gets, brush layers included, so a clone with a
     blend now goes aside in both renderers (`worn` in the GPU backend:
     it wears its mask and opacity on its strokes, as one with effects
     does).
     Written down with it: the browser suite now refuses to start on an
     `app/dist` older than any engine or UI source, naming the file — the
     stale build that let the last chunk's broken check read green.
     **And then the brush strokes themselves leave as curves.** With the
     edge where the stroke is, what a hard stroke covers is a shape a
     path says exactly: for each segment, a band whose sides run
     straight from one end's radius to the other's, with a half disc on
     each end — nothing more, since past an end the renderer holds that
     end's radius — and a stroke is the union of its segments', one path
     wound one way under the nonzero rule (`codecs/src/strokes.rs`, the
     half discs as quarter-turn cubics, off the circle by three
     ten-thousandths of the radius). An SVG writes each stroke as a path
     in its colour inside the layer's group, and a reader drawing it four
     times larger now agrees with the page pixel for pixel
     (`a_hard_brush_stays_sharp_however_large_it_is_drawn`); it was a
     picture at the page's resolution, which Chromium, reading the
     random pages at 4×, had shown smeared. A PDF draws the same paths
     live — each stroke in a state of its own, since a translucent
     stroke's alpha is graphics state and otherwise faded the opaque one
     after it (the only test that noticed, when that was taken out, was
     the one written for it) — so long as the layer's fade and blend are
     taken once: over one stroke, or not at all
     (`a_hard_brush_goes_live_as_the_curves_it_covers`). A soft stroke,
     an eraser or a stroke confined to a region still goes as pixels. Of
     the 472 brush layers on the reader audit's 800 pages, 161 now go as
     curves, and every audit holds — resvg's, ghostscript's, and the
     round trip back in.
     **And what composites as one picture stays live in a PDF too.** A
     faded or blended group, frame or copy — and, since the last entry,
     a brush layer of several strokes — went into every PDF as pixels,
     because drawn one part at a time each part took the fade, twice
     where two overlapped, and blended against the others. PDF says the
     engine's own rule for these directly: a transparency group. Such a
     layer is drawn into a form of its own (`/Group << /S /Transparency
     /I true >>`, blending in the page's colour — ink in ink), isolated as
     the engine isolates it, and that form laid down once with the fade
     and the blend (`draw_node`, `forms`, written in `finish` once the
     resources every stream shares are known). Vectors, type and brush
     strokes inside stay vectors, type and curves. Worth knowing for the
     next audit: the pages nobody wrote could not have seen the doubled
     fade — a quarter's difference in coverage, under the half those
     pages are held to — so it is held by a test of its own that reads
     ghostscript against the engine to two hundredths
     (`a_faded_group_goes_live_as_one_transparency_group`).
     MuPDF, reading the same page, agrees.
     **And a mask, too — which turned up the engine taking it twice.** A
     masked layer went into every PDF as pixels. It is now a transparency
     group laid down under a soft mask (`soft_mask`: a luminosity group
     drawing the engine's own picture of the mask, `mask_pixels_at`, at
     the density the PDF's other pictures are drawn for print — at one
     pixel a unit a reader enlarged it into steps). A shape, a picture or
     type keeps the fade it takes as it paints; a group, a copy or a brush
     layer takes its fade once, as it lands. Held to three hundredths of
     ghostscript across a feathered edge
     (`a_masked_layer_goes_live_under_a_soft_mask`), which found two
     things, neither in the PDF:
     - **Every exported mask was too light at a feathered edge.** The
       picture of a mask was worked out over the layer's box alone, and
       the blur found nothing past it: 0.78 at the edge where the page
       says 0.47. It takes in the blur's reach now, in the SVG as in the
       PDF.
     - **The engine masked a shape twice where its stroke lies over its
       fill**, and blended it twice there: the fill and then the stroke
       were each masked and blended as they painted, so an opaque shape
       under a mask letting through 0.475 came out 0.724 in the overlap.
       A mask says what of the *layer* shows. Such a shape is laid aside
       now and comes down once through its mask with its blend
       (`a_shapes_mask_and_blend_are_taken_once`), as the SVG had always
       drawn it; the audits, held to half a pixel, could not see a
       quarter. The GPU backend took the blend once already and the mask
       per paint, like the reference; the random pages found the two
       parting on the first page where it mattered (seed 766, a
       rotated, feathered, stroked ellipse), and a shape like that goes on
       a surface of its own there now, masked as it comes down.
     The fade stays as it was on purpose: a shape's opacity is taken as
     each paint goes down, by the engine's own long-standing rule.
     **And a layer held to the one under it.** A run of held layers went
     into every PDF as one picture. The engine confines a held layer to
     the alpha of the layer under it as drawn (`Cover`), and PDF can say
     exactly that: a soft mask read by its *alpha* (`/S /Alpha`) whose
     group is the layer under it drawn again (`as_group`). Set in the
     space the two share, before the held layer's own placement, with
     the held layer laid down as one isolated group under it — so a fill
     and a stroke over it are held once, and whatever the layer under it
     is (faded, masked, a group) is what holds (`draw_siblings`,
     `a_held_layer_goes_live_under_the_alpha_of_the_one_under_it`, read
     against ghostscript to three hundredths). A layer held to one that
     draws nothing draws nothing, as on the page. Taken out one at a
     time, the hold, the space it is set in, the isolation and the
     hidden base each fail a test. A held layer wearing a mask of its own
     wants two soft masks, which one graphics state cannot carry; they
     are nested — its own mask on a group of its own, the hold, fade and
     blend on the group around that — so it is masked and then held, as
     the engine cuts it, and stays live too
     (`a_held_layer_with_a_mask_of_its_own_goes_live`; un-nested, the own
     mask replaces the hold and the layer shows past the edge it is held
     to, 0.70 where the page has nothing).
     **And a layer wearing effects.** A shadow, an outline or an inner
     shadow sent the whole layer into the PDF as a picture, so text with a
     shadow was not text. PDF has no blur, so the effects stay a picture —
     but only the effects: the engine draws them beside the layer, the
     ones that go under it before it and an inner shadow after it, each
     with the layer's blend (`draw_layer`), and it now hands an exporter
     exactly that picture (`effect_pixels`, drawn from the same
     silhouette, in the space the layer sits in so that a copy's land
     where the copy is). The layer goes live between them
     (`effect_image`;
     `a_layer_with_effects_stays_live_beside_a_picture_of_them`). Read
     against ghostscript for what the effects *add* — the same page
     without them is the baseline, since ghostscript sets a letter's edge
     a fifth lighter whatever is behind it — and, for the inner shadow,
     which covers nothing the layer did not, by colour: it darkens the
     tile's corner in both and its middle in neither. How much it darkens
     differs twofold, which is the linear-light mixing above and not
     placement. Two exclusions: with a blend, more than one effect on a
     side goes as pixels, since as one picture they would blend with each
     other; and a held layer with effects, since the engine grows those
     from the silhouette already cut. What a layer is held *by* is drawn
     without its own effects (`plain`), the engine's `Cover` being the
     layer's alpha before them — written first as a refusal, which no
     test noticed taken out, and then as the fix with a test that does
     (`a_hold_reads_the_layer_under_it_without_its_shadow`: without it a
     held layer shows over the shadow).
     **And gradients.** Every shape with a gradient went into the PDF as
     pixels. It is a shading now (`gradient_fill`): the gradient's
     geometry is written against the shape's unit box, which a `cm`
     carries onto the box (`gradient_box`), clipped to the shape; its
     colours are the engine's own ramp read off at 256 points into a
     sampled function (`ramp_color`, which the GPU backend bakes the same
     way), so the file mixes between stops exactly as the page does rather
     than by PDF's own interpolation; where the ramp's alpha varies, a
     soft mask of the same ramp's alpha. Read against ghostscript by
     colour inside a three-stop linear gradient (to four levels) and by
     coverage inside a radial one with a translucent stop
     (`a_gradient_goes_live_as_a_shading_of_the_engines_ramp`; a two-stop
     interpolation, the alpha mask, the box's mapping each fail it taken
     out). The tests that had used a gradient as their example of what
     needs pixels use a tapered stroke now.
     And one thing the last entries left behind: the engine had begun
     blending a stroked shape once over its fill and stroke together, but
     the PDF still blended each paint, so where the stroke lay over the
     fill it was blended with the fill too — a multiplied red stroke over
     a blue fill on grey came out black there instead of red times grey.
     Such a shape is a transparency group with its blend on the group now
     (`a_blended_shape_meets_the_page_once_in_a_pdf`). Coverage could not
     see it; colour does. And `ghostscript_rgba` takes a directory of its
     own a call: two tests running side by side had read each other's
     pictures. In ink, a gradient's ramp is separated through the press
     profile like every other colour and the shading is four inks in the
     profile's space (`a_gradient_in_ink_is_a_shading_in_the_press_profile`,
     which self-skips without `CHITRAKAR_TEST_CMYK_ICC`).
     **And strokes, as the engine draws them.** A stroke whose width
     varies along it — a pressure stroke, a taper — went into the PDF as
     pixels and into the SVG *as wide as its nominal width all the way
     along*: the SVG writer never read `widths`. Both now write the region
     it covers, filled. The engine already states that region as convex
     pieces whose union it is (`stroke_pieces`: a band between two points
     whose sides run straight from one half-width to the other, a disc at
     a round end or join, a polygon at a square end or a carried corner),
     and each is one outline, all wound alike (`piece_outlines`,
     `tapered_outlines`) — a mitred corner wound the other way would have
     been cut out of the stroke where it overlaps, which only a test with
     square ends and mitred corners sees
     (`every_piece_of_a_tapered_stroke_winds_alike`). Only a path tapers: a
     rect's and an ellipse's stroke take no widths. Two more SVG defects
     came with it, both colour rather than coverage and so invisible to the
     half-pixel audit: **a translucent stroke went into every SVG solid**
     (its alpha was dropped from its colour, like a fill's, and written
     nowhere else — there was no `stroke-opacity` in the writer at all),
     and **a faded shape was faded once**, `opacity` over fill and stroke
     together, where the engine fades each paint, so where the stroke lay
     over the fill the SVG was lighter. The fade goes on each paint now
     (`fill-opacity`, `stroke-opacity`); mask and blend stay on the layer.
     Resvg drawn four times larger and the engine agree to five
     thousandths on a faded, translucent, tapered stroke over its fill
     (`a_tapered_translucent_stroke_leaves_as_the_engine_draws_it`), and
     ghostscript to three hundredths
     (`a_tapered_stroke_goes_live_as_the_outline_it_covers`). The PDF
     tests' stand-in for "needs pixels" is a soft brush dab now, the
     last ordinary thing a PDF cannot say.
     **And colour, where coverage cannot see.** Every audit so far
     asked only *where* a reader paints, because where two readers mix
     translucent paint they mix in different light and colour would be
     noise. So the pages are made opaque first
     (`chitrakar_doc::fixture::opaque_page`: every colour at full alpha,
     every layer plain, brushes hard, what only changes what is under it
     hidden), and then the colour of every pixel the engine paints flat
     and opaque has to be what the reader paints, within twenty levels
     (`a_reader_colours_an_opaque_page_as_the_engine_does`, 800 pages in
     resvg; `ghostscript_colours_an_opaque_page_as_the_engine_does`, 300
     in ghostscript). Two things it found, both in what a file holds
     rather than how a reader takes it. **A clone in a frame was pictured
     lifting the frame's ground**: an upright frame is drawn in place, so
     its clone lifts the page under it — beyond its box too — but
     `clone_alone`, which draws the clone aside for its thumbnail and for
     what SVG and PDF lay for it, took every frame to be isolated like a
     group, and the exported page had a grey patch where the page had
     purple (`a_clone_in_a_frame_is_pictured_lifting_what_the_page_lifts`).
     And **every picture in a PDF was dark along its edges**: a PDF image
     keeps its alpha apart as a soft mask, a reader that smooths a
     picture as it scales it smooths the two apart, and the colour under
     the transparent pixels was black — a layer whose edge fell half a
     pixel in was a quarter darker along it. The painted colour is
     carried eight pixels out into what is not painted now (`bleed`),
     one ring at a time, which costs the edge rather than the page. What
     the two still allow is two pixels a page: a picture of a few pixels
     at a fractional place, which each reader resamples its own way, and
     a mark thinner than a pixel, which ghostscript paints whole.
     **And the round trip, in colour.** The same question asked of an
     SVG exported and brought back in, where both sides are the engine's
     and so every flat pixel is held to its colour, translucent ones too
     (`a_page_exported_and_brought_back_is_the_colour_it_was`, 400
     pages). One page in four hundred was wrong, and what it found ran
     well past the importer: **`combine` says `None` for an answer with
     nothing in it as well as for one it cannot trace**, and every caller
     read the second. In the importer, a clip inside a clip it does not
     meet kept the inner one — a frame standing wholly outside the frame
     it sits in came back whole, over its neighbour. In the editor,
     keeping the overlap of two boxes that do not meet, or taking away a
     box that covers all that is picked, was refused with an alert about
     edges that overlap exactly; and a box *snapped* to the selection's
     edge and covering it, taken away, left a selection one
     five-hundredth of a pixel thick — the nudge's own sliver — that no
     one could see and every brush was confined to. A box dragged from
     its middle with alt held lands touching the first as often as not,
     which is the case the arithmetic finds degenerate; the browser
     suite found that one where the engine's own test, two boxes apart,
     had passed. `combine_or_nudge` now answers `Some` of no rings for
     nothing (`leaves_nothing`, the same pieces `combine` keeps, asked
     whether there are any) and drops rings thinner than two nudges, so
     each caller gets nothing as nothing: the selection is let go of, one
     step and undoable (9bz in the browser suite); a shape boolean says
     "nothing would be left"; the importer leaves out what shows nowhere
     — which is also an empty clip path, and a mask drawn outside its
     own rectangle, both of which had let everything through. Found on
     the way: **a clip on a clip path and a mask on a mask were never
     applied** — looked for on their contents, where usvg never puts
     them, rather than as `ClipPath::clip_path` and `Mask::mask` — and
     are now, in the referring group's space as resvg applies them
     (`a_clip_on_a_clip_path_narrows_it_as_a_reader_does`).
     **And a PDF of the frames, page by page.** Each frame's page held
     to what exporting that frame as a picture gives
     (`artboard_pixels`), in colour, on the opaque pages
     (`each_frames_page_is_the_frame_as_its_own_picture`): fourteen of
     seventy-six frames came out wrong, every one mostly white. A frame
     *held* to the layer under it was put on its page with every other
     layer hidden, that one included, so it showed nothing; it is let go
     of for its own page now, as the picture export already did. And
     what goes as pixels was rendered over the document's own rectangle,
     through the page's edge, so a frame reaching past the document's
     edge was cut off there on its own page — while its live vectors
     carried on past it. `place_rendered` renders over the area the page
     shows (`Page::area`: the document, or the frame's box) with
     `render_past_the_page_at`, the document drawn without its edge;
     for a whole-document page the two are the same rectangle. And a
     *turned* frame's page was the upright box around it — the frame at
     its angle in the middle, corners of nothing round it — where the
     picture export stands it upright, "since what is wanted is its
     contents, not its angle". A page carries a view now (`Page::view`,
     `size`: the identity and the document for a page of the whole; for
     a frame, its placement undone and its size on the page put back),
     folded into the page's one transform to points, and what goes as
     pixels is rendered through it. The audit takes turned and scaled
     frames too: 110 frames, where the upright ones were 76.
     **And ink.** A press PDF read back by ghostscript, against the
     engine's page soft-proofed through the same profile, on the opaque
     pages (`ghostscript_reads_ink_as_the_proof_shows_it`, which needs
     `CHITRAKAR_TEST_CMYK_ICC`). At first a third of the pages disagreed,
     every shadow deeper — ghostscript compensates for black by default
     and the proof does not; asked not to (`-dBlackPtComp=0`) the median
     difference is nought. What was left was real: **a page in ink had
     no transparency group of its own**, so ghostscript blended whatever
     a group brought down — a held layer, a faded group, a blended shape
     — in its screen's RGB, taking the ink there by its own route, and
     those layers came out deeper than the rest of the same page. Each
     page in ink is a transparency group in the press profile now
     (`/Group << /S /Transparency /CS … >>`), which is what a press file
     should say anyway. Five pixels a page are allowed, for hairlines of
     self-crossing paths that ghostscript paints heavier.
     **And files written elsewhere, coming in.** The round trip only ever
     handed the importer what this exporter writes. A generator of SVG
     files nobody here wrote (`foreign_svg_with`: every basic shape, paths
     with relative commands, quadratics and arcs, every kind of
     transform, dashes, caps and joins, clip paths in both unit systems,
     `use`, nested viewports) holds the engine's picture of what came in
     to resvg's picture of the file
     (`a_file_written_elsewhere_comes_in_as_a_reader_draws_it`). Asked by
     feature, one at a time, a third of files with single plain shapes
     were wrong, and so on up. **An outline crossing itself** — a star, a
     looping polyline — came in with a hole wherever it wound round
     twice, since SVG fills by winding, the engine even-odd, and the
     importer converted only rings that overlapped each other. It is cut
     where it crosses itself now and kept where it is the edge of what
     winding fills (`boolean::nonzero_as_even_odd`), and left with its
     curves when the two rules agree; where outlines only touch — a
     tapered stroke's bands and the discs that cap them, which our own
     exporter writes — that cannot be traced, and the old union of
     rings wound alike, which nudges touching edges apart, answers
     instead (the round trip caught that one); where such a path is stroked too it
     comes in as its fill and then its stroke along the file's own line
     (`shapes_of`), since a stroke laid round the converted edge closed
     open paths and turned crossings into corners. **`stroke-dashoffset`
     had nowhere to land**: `Stroke::dash_offset` now (additive, nought
     for every older file), walked into the pattern by `dashed_rings`
     with SVG's rule — round the pattern's period, which is twice its sum
     for an odd number of lengths — written as `stroke-dashoffset` and as
     a PDF dash's phase, read on import, set in the fixture from the
     pattern it already drew so no page moved, and offered beside the
     line pattern in the panel (8k1). **Dashes along a curve had round,
     swollen ends**: a round join was a disc at every point a curve is
     flattened into, reaching past a dash's flat end; a join that only
     bends a little (`SLIGHT_TURN`, fifteen degrees) is filled as a
     miter, which differs from the disc by a three-hundredth of the
     half-width. And **two ends a hair apart either side of a grid
     cell's edge did not meet** in `chain`, so a crossing worked out from
     each of its two edges could leave an outline unclosable; ends meet
     within a weld by distance now, through the cells round them.
     **Then gradients.** Colour compared only where a picture is flat
     had let every gradient through — a gradient is nowhere flat — so
     they are held everywhere the shape is solid, one shape a file
     (`a_gradient_written_elsewhere_comes_in_painting_what_a_reader_paints`).
     **Every radial gradient came in wrong**: as a circle in the box's own
     units with the file's radius over the box's half-diagonal — half as
     wide again on a square, an ellipse on anything else. The engine's
     radial takes axes of its own now (`Gradient::Radial::axes`, a 2×2
     matrix the offset from the centre goes through; `None`, every older
     file, the identity), the file's rings come in through them exactly
     (`M⁻¹ B / r`), SVG writes them as a gradient transform and PDF as one
     more `cm` before the shading, and the GPU draws it by handing the
     shader the box's coordinates already through the axes and the
     centre with them — the ramp is `|A·uv − A·c| / r`, so the shader
     measures as it always has — set at a covered path's corners, which a
     linear map carries exactly; a rectangle or an ellipse wearing one is
     filled the way a path is
     (`a_radial_with_axes_of_its_own_ramps_the_way_the_cpu_ramps_it`).
     Worth knowing: a container without a GPU adapter *skips* every GPU
     test with one line of output, and a fresh one has none until
     `mesa-vulkan-drivers` is installed (llvmpipe) — two commits' gates
     ran without them before that was noticed; the crate passed when it
     was. A **linear** one in user
     space over a box that is not square came in with its bands turned —
     its ends taken into the box straight across, where the engine lays
     bands square to the line in the box; the ramp itself is carried
     over now (`t = (q − q1)·B a`). And a curve bulging past its anchors
     had its gradient laid over the anchors' box rather than the curve's
     (`gradient_box`). Found on the way, and not ours: ghostscript draws
     a radial shading up to a device pixel further out than its rings,
     with axes or without, so the PDF witness compares where along the
     ramp a pixel is, drawn large.
     **And past the ends.** A gradient's `spreadMethod` — reflect,
     repeat — had no field and came in padded: `Gradient::spread` now
     (`Spread::{Pad, Reflect, Repeat}`, additive, `Spread::place` the one
     statement of it), on both kinds. The CPU and the GPU place the ramp
     position through it (the GPU's spread rides in a colour slot a
     gradient left empty), keeping a line or a ring of no size the end
     colour it always was; SVG writes `spreadMethod`; and PDF, whose own
     extension only pads, stretches the shading over every whole period
     the shape's box reaches — a line's ends pushed out, a ring's reach
     grown — with the spread already applied to what its function holds,
     a pad writing exactly what it wrote before. The panel offers it
     beside the gradient (the block that walks gradients in the browser
     suite reads one pixel past a ring's rim under all three), and the
     angle slider keeps it rather than rebuilding the gradient bare.
     Held against the CPU on the GPU, against resvg in SVG and against
     ghostscript in PDF, each failing when its own layer is undone.
     **And from a focus.** A radial's `fx`/`fy` — rings starting from a
     point off the centre, a highlight to one side — came in starting
     from the centre: `Gradient::Radial::focus` now (additive, in the
     centre's units, so the importer carries it in the way it carries
     the centre). `focal_offset` measures it from the centre through the
     axes in radii, holding a focus at or past the outer ring just inside
     it (`FOCUS_REACH`, SVG 1.1's rule: SVG 2 draws a cone there that
     readers disagree on), and `focal_ramp` is the one statement of which
     ring a point is on — the root of `k t² − 2(w·f)t − |w|² = 0`, said
     so the small answers behind the focus keep their digits; the CPU,
     the shader (the focus rides in the radial's spare geometry slot and
     a colour slot) and PDF's reach all use it. Rings with no focus keep
     the arithmetic they always had, to the bit: the same distance
     divided in another order moved a group's drop shadow by 3e-4 in the
     wrap-a-layer-in-a-group test, past its 1e-4 — a last-bit change
     amplified somewhere on the way through the shadow, not chased. SVG writes the held
     focus as `fx`/`fy`; PDF's shading starts as a circle of no size at
     the focus and ends `t1` rings out, its middle carried past the
     centre as far as that takes it. `fixture::focal_page` — a focus
     beside plain rings repeated, one inside turned axes reflected, one
     set past the ring — is the page resvg, ghostscript (away from the
     pixel beside a focus held at the ring, where the ramp runs its
     length in a pixel) and the GPU are each held to, each failing when
     its layer is undone; the random pages give every radial a focus
     too, and the importer's gradient test now draws one in every file.
     On the canvas, shift-dragging a radial's centre knob pulls a focus
     out of it (an accent-filled knob); the centre carries the focus with
     it, and a focus dragged back onto the centre is none. Writing that
     step found a real bug: Shift held down on a knob stretched the
     page's text selection to it, and the next drag picked the
     selection up as a native drag — `pointercancel` after one move, the
     knob stuck and the gesture never committed. The gradient knobs'
     pointerdown now prevents the default, and the canvas host is
     `user-select: none` (its in-place text box excepted) so the other
     handles that read Shift cannot start one either.
     **And a pen that leans with its path.** A stroke under a skew or
     an uneven scale came in with a round pen of one width — the path
     brought into page space and its pen with it, where a reader skews
     and stretches the pen with the path: with transforms let into the
     stroked files, 50 of 400 were over ten pixels out and the worst
     125. A stroked path whose transform is not a similarity is kept in
     its own space now, the transform on the layer (`own_space`; the
     layer's mask stays in page space, where masks live), so its pen,
     its dashes and its gradient are the file's as written; everything
     else still comes in in page space as it did. Strokes are taken
     under every transform in the import test now, inside its old
     bounds (the worst file 20 pixels).
     **And a pattern, painted.** A shape filled with a `<pattern>` came
     in with no fill at all — `Paint::Pattern` had no answer — so a
     hatched or textured shape arrived as an empty outline. The engine
     has no pattern fill (a tile repeated under a transform of its own),
     and adding one is a new paint everywhere; so the importer does what
     a reader does — draws the tile once at the scale it is seen at,
     here with the engine's own renderer, its contents brought in by
     `walk` like any of the file's — and lays it across the shape's box
     at a document pixel a pixel (sampled through the pattern's
     transform, wrapped, bilinear), as a picture whose mask is the
     shape's outline (`pattern_picture`); a stroke over it still comes
     in as the vector it is. Above sixteen megapixels the picture is
     drawn coarser rather than refused. Writing it found usvg's own
     TODO: a pattern in the box's units has its contents wrapped in a
     group whose transform the paths under it are never told about, so
     every one stood where it would with none; `walk` takes the truth
     of where a group stands now, and puts each layer under it right by
     the difference. Four files — stripes in user space, a checker
     turned by `patternTransform`, dots in box units through a viewBox
     under a stroke, and a skewed ellipse in a scaled group — are held
     to resvg pixel by pixel (`a_pattern_fill_comes_in_painted`), and
     each fails with the group fix or the outline mask taken out.
     **And a mask with grey in it.** A `<mask>` drawn in anything but
     opaque white — a gradient fading a picture out, a half-opaque shape
     — was passed over, and what it should have faded came in whole.
     It is drawn now as a reader draws it (`soft_mask`): its contents
     brought in by `walk` and drawn by the engine in the space of the
     group wearing it, cut to the mask's rectangle, read as luminance
     times alpha on the values a device shows (or alpha alone), a mask
     on the mask multiplied in, and the region the group is also cut to
     folded in as well; kept as a raster mask, white with the coverage in
     alpha. It goes on a *group*: a fade does not distribute over the
     layers under it the way a clip does — two overlapping layers each
     faded to half are three quarters where they overlap — so the
     importer's output, flat until now, keeps the groups a file needs
     (`ImportedGroup`; a region-only mask still flattens as a clip), and
     `ImportedSvg::into_commands` is now the one place a file's layers,
     pictures and mask pixels become a document — the engine's placing,
     the tests' and the pattern tile's all go through it. Held to resvg
     on four files away from either renderer's edges
     (`a_soft_mask_fades_what_it_covers`); ignoring the mask, reading
     luminance as alpha, dropping the folded-in clip, and putting the
     mask on each layer instead of the group each fail it. Shades that
     meet inside one pixel of a mask mix in linear light here and in a
     device's values in resvg, which is the engine's compositing and was
     left. **And a faded group, faded as one.** Group *opacity* had
     always been folded into each layer's colours — the same
     non-distributing approximation the mask nearly made, so two
     overlapping rectangles in a group at half came in three quarters
     opaque where they met. A faded group with more than one layer under
     it (`draws_more_than_one`) is an `ImportedGroup` wearing the fade
     now, whatever was folded above it coming onto it too; a fade over
     one layer still goes into that layer, where it is exact, and adds no
     group the file did not need. Held to resvg on three files — two
     overlapping layers, a faded group inside a faded group, a faded
     group cut to a clip with a stroke over a fill — and failing with
     the fold put back (`a_faded_group_fades_as_one`); the browser suite
     reads half, not three quarters, where two layers of a placed file
     overlap.
     **And a blend, both ways.** `mix-blend-mode` — which the exporter
     has always written — came back Normal. A blended element with one
     layer under it gives that layer its blend; with more, or with a
     blend inside it as well (two blends are two steps), it is an
     `ImportedGroup` wearing it; and a group a reader isolates (a clip, a
     fade, a mask, `isolation:isolate`) with a blend inside keeps it
     inside (`a_blend_comes_in_as_the_file_says_it`). Letting masks,
     blends, fades and held layers back into the export round trip
     (`portable`, 400 pages, coverage and colour) then found three bugs
     that were not the importer's. The exporter wrote a group whose
     layers blend as a plain `<g>`, which SVG does not isolate, so a
     Darken layer in a group darkened the whole page in every reader:
     such a group, frame or copy says `isolation:isolate` now, on the
     engine's own terms (a copy only where its stand-ins read the page).
     A masked or held layer's wrapper isolated it, so its blend met
     nothing: the outermost wrapper takes the blend now, as the effects
     wrapper always did. And the *engine* drew a blended frame inside a
     blended frame as nothing but the page under it: a frame whose
     contents read the page was sent down the road for contents that
     work on it — an adjustment, a clone — drawn in place and alongside,
     the difference added back as what they made of the page, and for a
     blend that difference is the blend meeting the page. For a frame
     with a blend of its own that road is taken now only for what works
     on the page (`reads_in_place`, `works_on_what_is_under`); a frame
     merely faded or masked still draws its blends where it stands, so
     a mask that hides nothing still changes nothing — which the first
     draft of this broke, and `a_mask_that_hides_nothing_is_no_mask`
     said so. And a hidden layer works on nothing — a hidden adjustment
     had sent a frame down that road too
     (`a_blended_frame_in_a_blended_frame_draws_what_two_groups_draw`).
     A reader is held to the blended pages as well now
     (`a_reader_blends_an_opaque_page_as_the_engine_does`, 800 pages),
     failing with either export fix undone, and
     `a_masked_layer_keeps_its_blend_in_svg` for the wrapper. Left out of
     it, each said there: the four non-separable modes, where resvg's
     arithmetic is not the W3C's (a blue brush at Luminosity over gold is
     [84, 60, 0] by hand from SetLum and ClipColor, the engine's answer;
     resvg draws [115, 60, 0]); frames and copies holding what works on
     the page, the engine's own construction; and blends inside frames,
     which was a choice to make, since made (below). Clone layers were left out
     too at first, a clone lifting from under a blended copy travelling
     as a picture that disagreed with the page; chased, it was two bugs,
     neither about blends. A clone drawn aside to be exported, as its
     thumbnail and its SVG picture are (`clone_alone`), laid its strokes
     over all the room drawn past the page, so a stroke reaching past
     the page's edge left paint out there and a later stroke lifting
     from past the edge lifted it — a patch the page never showed; they
     are laid only where the page is now. And a copy of a clone layer
     went into an SVG brought down Normal, where the engine brings it
     down by the clone's own blend (`copied_blend`). Clone layers are on
     the blended pages now, and each fix fails them undone. PDF was
     asked the same of ghostscript
     (`ghostscript_blends_an_opaque_page_as_the_engine_does`, 300 pages,
     all sixteen modes, blends inside frames as well) and had the same
     first bug: a group holding a blended layer was drawn straight onto
     the page, so a Saturation layer in a group took its colour from the
     page; it is an isolated transparency group now, on the engine's
     terms, and fails undone. A frame goes to PDF as a clip, not a group,
     so a blend inside a plain frame reaches the page there as it does
     on the engine's — PDF already says what SVG could not. The choice
     about frames — the engine draws a plain frame where it stands, so a
     blend inside it reaches the page, while a frame travelled to SVG as
     a clipped group, which every reader isolates — was put to the
     owner and taken the way that changes no picture in the editor: the
     export says pass-through now. A plain upright frame holding blended
     paint (`passes_through`) wears no cut of its own; each layer in it
     wears the frame's, outermost, with its blend on that wrapper, as a
     mask's wrapper takes it. A frame holding what works on the page, or
     another such frame (whose cut would be read in the wrong space),
     goes as before. The SVG reader check takes blends inside frames now
     and fails with the old export put back (page 408: a Multiply layer
     in a plain frame).
     **And a shadow.** Every SVG `filter` was passed over, so a card with
     a drop shadow came in without one. A filter is read now for the
     engine's three effects (`effects_of`): `feDropShadow`; the chain
     this editor's exporter writes for a drop shadow, an outline and an
     inner shadow, merged under and over the source; and the chain design
     tools export — hard alpha, offset, blur, the shape knocked out, a
     colour matrix laying the colour, blended in under the source,
     several deep. Offsets and blurs go from the filter's space to the
     page's, and a blur becomes the engine blur whose exported spread
     (`√(r(r+1))` at its box radius) is nearest, so every blur the
     engine has comes back at its own radius
     (`a_blur_exported_comes_back_at_its_radius`). On one layer the
     effects are its own; on more, faded, or text (which arrives a glyph
     at a time) the group's. Anything else is passed over as before.
     Held to resvg on three files (`a_shadow_written_elsewhere_comes_in_
     as_a_shadow`), round-tripped whole (`an_effect_exported_comes_back_
     as_itself`), and let into the 400-page export round trip, which
     fails with the recogniser off — all but outlines: an outline's edge
     is where its layer is half as covered as its opacity, a vector's
     opacity goes to SVG as the fade of each paint and comes back as the
     colours' own, so a faded layer's outline came back with its edge at
     half the full cover its silhouette never reaches. The exporter
     writes that threshold; there is no field yet to give it back to.
     The colour round trip compares premultiplied now, since a shadow's
     faint tail is pixels whose own colour is rounding.
     **And an outline, faded.** The threshold is the field: the edge the
     exporter writes is half the layer's opacity, so `effects_of` reads
     the fade back from it, and the layer it lands on gets it as its
     opacity again, out of the paint it was folded into (`unfade`); a
     group kept for its effects — a faded block of text, or a shape
     whose tapered stroke went as a second path — takes it as its own
     and what is under it comes in unfaded. Exact for text, whose glyphs
     do not overlap; a fill and a stroke drawn as a second shape were
     two faded paints and are one faded group, a little lighter where
     both are. With that, outlines went into the 400-page round trip,
     and it found two more things, both real:
     the **engine would not outline a hairline**. An outline is measured
     from where a layer is half covered, and a line thinner than a pixel
     — a hairline path, the stem of a letter set small — is never half
     covered anywhere, so it cast no outline, or one in pieces wherever
     its smoothing happened to cross a half (the fixture's first page: a
     caption, outlined round its capital C and nothing else). The middle
     of such a line is a *ridge* across it — at least as covered as both
     neighbours along one axis, and more than one of them, both by a
     margin — and counts as inside now, in both renderers
     (`chitrakar_render`'s `ridge`, the shader's `field_cover`): a soft
     edge fading away is a slope, never a ridge, so a feathered picture's
     outline stays where it was, and a solid shape's ridges are past half
     already, so a solid outline does not move by a pixel
     (`a_line_thinner_than_a_pixel_is_outlined`,
     `a_soft_edge_is_outlined_where_it_is_half_covered`). The margin is
     not decoration: without it the level line of a soft edge was a
     ridge of rounding noise, and the GPU's noise is not the CPU's — the
     clone layer's outline audit caught it at once. A ridge counts from
     a fifth of the layer's opacity up: below that is where two honest
     drawings of one shape part company — text set by the text renderer
     and the same glyphs as paths, at a turned letter's faintest
     smoothing — and the SVG round trip, which brings text back as paths,
     found a tenth amplifying it on a copy of a turned caption (seed 221).
     On the way there the audit that
     adds an effect drawing nothing to every bare layer found **a copy of
     a clone layer whose silhouette changed with whether the clone it
     copies wore an effect** — a trace between a tenth and a fifth
     covered on seed 19, which the half-covered edge never saw and which
     a shadow on the copy showed as 0.025 in six pixels. Two layers
     wearing effects on the way from a copy down to its clone was more
     than `clone_behind` resolved, so the copy went to a surface of its
     own, where a clone has nothing to lift: its effects grew from a
     stray trace, and a real shadow on the clone was missing from the
     copy altogether. Each layer on the way is a level now — a copy's
     share, a layer's effects in its own space — laid inner to outer
     (`lay_levels`), the copy's effects grown from a picture that
     already holds the clone's, as a copy of any other layer is drawn
     (`a_copy_of_a_clone_wears_its_effects_round_the_clones`). The one
     wearer that every page had before goes the way it went. The GPU agrees pixel
     for pixel on upright and level hairlines and outlines a slanted one
     too, though not to the pixel: it covers a path with four samples,
     so a third of a pixel comes out a quarter or a half, and an outline
     measured from that inherits it
     (`a_hairline_is_outlined_as_the_cpu_outlines_it`). SVG keeps the
     plain half-covered step — a ridge is not something a filter can ask
     cheaply — so a hairline's outline does not reach an SVG reader; the
     engine draws it from the file again on the way back in.
     And a **group's effects over a layer with effects of its own lost
     one of the two**: a copy outlined round a layer casting a shadow
     came back with the shadow and no outline, because the group's
     effects were carried down to the one layer under it, which kept its
     own. Such a group is a group now (`holds_a_filter`), outlined round
     the shadow as the file draws it
     (`a_groups_effects_over_a_layers_own_come_back_both`). Not about
     outlines at all — a shadow over a shadow lost one the same way.
     Long dashed curves drift a pixel or so against resvg; finer flattening
     made it worse, so the difference is the reader's arc length as much
     as ours, and it was left.
     The twenty-sixth was **a second picture on the first one's bytes,
     standing turned** (`again`), which is two things this document had
     never held. Every resource in it was referred to exactly *once*, so
     a saver writing a PNG per reference, or a loader handing the second
     layer a copy of its own, would have passed everything. And no
     *raster* stood turned: the twenty-fifth's stroked rectangle is a
     vector, and a turned picture is sampled through the inverse of a
     rotation rather than read along its own rows.
     Every audit held, so the code was broken, and the turn's half was
     measured the way the twenty-fifth's was: sample a raster through
     the *transposed* turn — a change that does nothing at all to an
     upright one — and
     `every_layer_of_the_fixture_puts_down_what_the_cpu_puts_down`
     **passes without this layer and fails with it**. Earned outright,
     and by the same instrument twice, which is worth noting: the
     per-layer cross-renderer audit is where a new kind of placement
     pays.
     The sharing's half earned a test rather than a sabotage
     (`two_layers_on_one_set_of_bytes_travel_as_one`): one file per
     resource named for it, and both layers naming the same resource on
     the way back. Its first draft compared *counts* and a saver writing
     a file per reference slipped straight past it — two references, two
     files, the right number and the wrong files — so it compares names
     now. A test written to catch a particular mistake should be shown
     the mistake before it is believed.
     The twenty-seventh was **type set loosely** (`spaced`): tracking,
     and a line height of its own, which no block here had — every one
     was spaced the way its face spaces itself. The first try put both
     on the wrapped block, and the method caught it: a wrap that forgot
     the tracking was noticed only by the text module's own test, with
     the new spacing and without it alike, because at eighteen points
     that block's two words are wider than the page before tracking
     comes into it — the break could not move. So the spacing went on a
     block of its own, *placed so the tracking decides the break*: at
     twelve points "Agile mark" is fifty-five and a half pixels set
     tight and sixty-four tracked, on a sixty-pixel line, so the right
     answer is two lines and the wrong one is one. Its edit takes the
     spacing back out, which changes the box's shape as well as its
     size.
     Then the sabotages, both measured against the fixture without the
     block. A wrap that forgets the tracking — still only the text
     module's own test, **with the block as without it**. That is not
     the placement failing a second time but the nineteenth's finding
     again, and worth having twice: every audit over this document
     compares it with *itself* by another road — the file, the wire, the
     clipboard, the second renderer — and every road shares one layout,
     so a layout that is wrong everywhere is wrong in agreement. Where a
     line breaks is held by tests that know where it should, and only by
     them. A file that forgets the tracking, though, fails two file
     audits with the block and **nothing at all without it** — the
     tracking had no witness in the file before. So the shape earns the
     round trip outright, and the honest account of the wrap is that no
     self-comparison can hold it.
     The twenty-eighth was **a stroke that heals**, the second stroke on
     the clone layer (`borrowed`). Every stroke here had cloned; none had
     healed, and a heal is the one stroke whose every pixel depends on
     every other — its colour is the difference of two averages over
     the whole of it. It found two defects in the reference renderer
     before the backend was touched.
     The first, with the stroke and not without it:
     `a_dissolved_group_brings_its_children_masks_with_it` saw the page
     move by 3.5e-4 when a shape came out of a group, against a
     ten-thousandth everywhere else. The heal averaged **straight**
     colours, every covered pixel counting as much as any other — and
     the straight colour of a pixel with next to no alpha is whatever its
     last few bits say, so the noise of composing the same transforms in
     another order was divided by those alphas and fed into every pixel
     of the heal. The averages are premultiplied sums over alpha now, so
     a pixel a five-hundredth there is a five-hundredth of the answer
     (`a_pixel_that_is_barely_there_does_not_decide_what_a_heal_takes`:
     half a stroke over solid grey and half over a wash of red at that
     strength came out pink, 221 against 130).
     The second, which the fixture could not hold: `filter_reach` gave
     a clone layer the reach of its offset, and a heal reaches its own
     length further — a region render padded by the offset alone ends
     partway along a heal and averages the part it drew. The fixture's
     own blur pads further than its heal reaches, so it passed either
     way; a heal on a page of its own, long and read from close by, is
     now a case of `a_region_render_padded_by_the_reach_is_the_page`.
     Then the backend, which handed every heal back — the last thing a
     layer could *hold* that it did not draw, bar a band wider than a
     pass will walk. It draws them now (`Healing`): a pass writes each
     pixel's share of the two sums into a pair of full-precision
     textures over the stroke's rectangle, passes sixteen by sixteen add
     them to one texel, and a last one writes the shift beside the
     stroke's coverage, which is what the clone pass lays from. Four
     sabotages, and which tests noticed: a lay that ignores the shift —
     the clone audit, whose every case now also runs healing, and the new
     `a_heal_averages_what_the_cpu_averages`; a sum that adds eight by
     eight — the clone audit; terms that ignore the stroke's region, that
     average straight colours, or that ignore a frame's cut — only the
     new test, one case each, and each read on both renderers rather
     than compared. **The fixture audits noticed none of the four.** The
     heal lies under enough of the page that six pixels of it show, and
     none by more than 0.09 — which is the fixture doing its job, finding
     the two defects above by carrying the stroke through every road, and
     not a job it can do for what the stroke's colour should be.
     Holding the rectangle to the page is only work saved, and the
     comment says so: off the page nothing is under the stroke, so a
     pixel there never counts, and no sabotage of it can show.
     The seventeenth was **an effect on a frame**, which could not have
     gone in an hour earlier: the backend handed such a page back, and
     one refused layer declines the whole fixture. Every effect in this
     document had hung on a layer or on a group, and a frame is neither —
     it is the one kind whose silhouette is not what it drew but the
     rectangle it cuts what it drew to.
     Two audits stopped holding, and neither was a defect. Both were
     tests whose *instrument* was narrower than they read.
     `every_kind_of_layer_survives_the_clipboard` excludes the bands near
     a page edge where an effect would be cut, and worked them out from
     where the layer was **sent** — but a paste is nudged so it does not
     hide behind the original, so a layer clearing the right and bottom
     edges by less than that nudge hangs off them once pasted. The frame
     sits at (56, 40) on a page 80 by 60 and clears every edge; its paste
     does not. The exclusion reads both places now.
     `a_copy_of_a_frame_cuts_and_grounds_where_the_copy_is_put` asked for
     an exact box of every pixel above zero alpha, and a blur has no such
     box: it reaches every way from what it blurs, and the far side picks
     up a tail — nine ten-thousandths of a channel here. **That tail
     nearly went down as a defect.** Drawn as ASCII with anything above
     zero as ink, it looks like structure: a band eight pixels wide on
     the side the shadow does *not* fall towards, and specks below the
     frame in a suspicious pattern. Reading the alpha instead of the
     picture ended it in one measurement. The threshold was the finding,
     not the renderer — the same shape of mistake as the fail-fast count
     and the "clippy ok" printed over a compile error, all three in one
     session, and all three the instrument rather than the thing.
     The test asks two sharper questions now instead of one loose one.
     The cut and the ground are asked of the frame with its shadow off,
     where the box is exact and stays exact. The shadow is asked as a
     *direction*: what the copy draws above 0.95 alpha is still the
     frame's box, since a shadow cast at 0.7 of a 0.9 colour cannot
     reach there and a ground is opaque; and the faint reach is longer up
     and to the left than down and to the right, which is what the offset
     decides and what no tail can fake.
     What the shape is worth, measured rather than assumed: break the
     backend's frame close again and **three** tests fail — the one
     written for it, and two audits over this document that could not
     have noticed before, since there was no effect on a frame here to
     notice with.
     **An effect that draws nothing is no effect** is the third of these,
     and it found a hole in a fix made earlier *this same session*. An
     effect is built from a silhouette, so a layer wearing one is staged
     on a surface and a layer wearing none is drawn straight — plainly
     different code, which is the test worth writing.
     A copy of an adjustment or a filter cannot go on a surface: what it
     copies rewrites what is under it, and there is nothing under a fresh
     one. That was answered for a copy sent to a surface by a **mask or a
     fade**, and an effect sends one just the same — which the answer
     missed. A copy of a filter wearing a drop shadow *at no opacity at
     all* still vanished, and so did copies of adjustments. Five pages of
     a hundred and fifty, the worst by 0.468.
     The first fix was wrong and the invariants said so, which is the part
     worth keeping. Drawing such a copy *straight after* its effects were
     taken from the surface — the shape of answer a clone layer with an
     effect gets — fixed the pages and broke two others: seed 11 of the
     mask invariant and, fatally, seed 1097 of the coverage one by 271
     pixels. The reason is general. Extending the condition to a copy of a
     **blended** layer makes the *target's blend* decide which path the
     copy takes, and then taking that blend off changes what the page
     covers. **A blend may not decide coverage.** Whether a layer rewrites
     what is under it asks no such question, so that half is safe and the
     blended half is a limit, written into the test as a named exclusion
     rather than a widened tolerance.
     The answer that held is the one the `blended` predicate beside it
     already takes: **ask what the layer draws, not what kind it is.** An
     adjustment and a filter are excluded from `effected` by name; a copy
     of one draws exactly what they draw, so it is excluded too, and the
     effect is ignored rather than given a surface — which is what the
     layer it copies has always got.
     A row of the backend's table of what it will draw turned from false
     to true with it: an adjustment or a filter wearing an effect is now
     *drawn*, by ignoring the effect as the reference always has. Handing
     a page back over an effect that changes nothing was a page declined
     for no reason.
     What is left is two pages of a hundred and fifty at 0.009 and 0.011,
     and they are worth a sharper description than "edge residue" because
     the obvious reading of them is wrong. **The layer wearing the no-op
     effect is drawn bit-identically** — rendered alone, with the effect
     and without, not one pixel differs on either page. What changes is
     what something *else* reads from it: on seed 129 a **copy** of that
     layer, and on seed 122 a **clip run** held to it. So the residue is
     not the layer's own edge at all; it is that a copy and a captured
     cover both go through the layer's surface, and an effect grows that
     surface's extent by its own reach even when it draws nothing.
     Under the threshold the committed test uses, and left there
     deliberately: the two readings are 0.009 and 0.011, and chasing them
     means changing how an extent is grown, which is four places this
     session has already had to fix once each. Written down with the
     measurement so the next attempt starts from it rather than from the
     word "edge".
     The filters were all clean: a blur of no radius, a sharpen of no
     amount, a motion blur of no distance, noise of no amount, a vignette
     of no amount and a pixelation of one pixel each leave the page alone
     over all hundred and fifty.
     The same question asked of everything *else* that touches a pixel
     found the same defect in two more places, and one of them is worse
     than the adjustments were. **Every blend but `Normal` crushed the
     highlights it met.** A blend is written in the display encoding, and
     that encoding stops at white, so the channels were held there before
     the blend function ever saw them: `Lighten` of a half grey over a
     ground at 2.415 came back **1.000** — a maximum that made the
     picture darker. Screen, Overlay, Difference and Luminosity the same.
     sRGB's curve is a power law and runs on past one happily, so the fix
     is to let it: below white the table still answers, above it the real
     function does, and the two meet at the join.
     Not by changing `linear_to_srgb`, which holds everything from white
     upwards *at* white on purpose — at one, `1.055 · 1 − 0.055` is a
     rounding short of one in f32, and colour burn's "is the backdrop
     white" branch has to be able to ask. That exactness is load-bearing
     and stays where it is; the blend path carries its own extension.
     And **sharpen**, which held its premultiplied channels to the alpha
     they are a share of — true of display-referred colour and false
     here. A ground two stops up came back at one, the filter throwing
     away what the exposure above it had been keeping. Blur, pixelate,
     vignette, the shadows and the outline were all clean.
     The sharpest assertion in the test is `Lighten`, because `max` can
     only be wrong one way: what comes back cannot be less than the
     brighter of the two, and no argument about encodings or rounding
     excuses less. That one line would have caught this on the day the
     blend path was written.
     Both renderers again, and again the cross-renderer audit was the
     only test that failed when the reference was fixed — twice over, once
     for the blends and once for sharpen. Nothing below white moved
     either time.
     The rule from that discarded invariant paid at once. **An adjustment
     at its neutral setting is no adjustment** — the two sides plainly go
     down different code, since with the layer the whole page under it
     runs through `apply_adjustment` a pixel at a time and without it
     nothing happens at all — and it found the worst defect of the
     session.
     **Seven of the eleven adjustments clamped every channel to one.** At
     their neutral settings, which are the app's own `ADJUSTMENT_PRESETS`
     defaults. This pipeline keeps highlights unbounded on purpose — an
     exposure of a couple of stops puts a channel at three, which is why
     the GPU backend stores `Rgba16Float` and why the interior reading
     scales by `max(1, |v|)` — and Brightness/Contrast, Hue/Saturation,
     White balance, Vibrance, Levels, Curves and Invert-at-nothing each
     threw all of it away. Add a Levels layer to a photograph with
     headroom, touch nothing, and two stops of it are gone for good. A
     non-destructive editor destroying something.
     It hid because every test of an adjustment used a page inside 0..1,
     where a clamp at one is invisible. The sweep that found it puts a
     stop and a half of exposure under the page first, and the test keeps
     that with a floor saying so: a hundred pixels must stand above 1.2 or
     the page cannot catch a clamp at all.
     The rule now, everywhere: **clamp below at zero, never above one.**
     Light below nothing is meaningless; light above white is what the
     rest of the engine is built to carry. Two are drawn over the display
     encoding, where there really is no graph past white — Curves and
     Invert — and those hold the part that fits, work on it, and carry the
     excess across: a curve's *gain at white* multiplies what is above it,
     so the diagonal is the identity and a curve pulling white down pulls
     the highlights with it, meeting exactly at one.
     Both renderers, since the shader had the same seven clamps written
     the same way, and the cross-renderer audit said so the moment the
     reference was fixed — it was the only test in the workspace that
     failed, which is that audit doing its job. Nothing below white moved
     on either side: every other test passed untouched, before and after.
     Three sabotages, one per shape of fix — the plain clamp, the levels
     clamp, and the curve's carry — and each moves the page by 1.8284.
     An invariant was written, measured, and **thrown away**, and what it
     cost to find that out is the useful part. **A group that isolates
     nothing is no group**: a plain group — opacity one, blend Normal, no
     mask, holding nothing that reads what is under it — wrapped round a
     run of layers must leave the page exactly as it was. It is
     definitional, it needs no second renderer, and it is cheap. Two
     hundred and forty-seven pages of six hundred qualified after the
     exclusions were made honest (a blend inside a group meets the
     group's contents and not the page, which is a real difference; a
     clip run must not cross the boundary, or its base is left outside),
     and it found nothing.
     Finding nothing is not the reason it was thrown away. It was thrown
     away because of what happened when the code it nominally watches was
     broken: **stop clip runs forming altogether and it still reports
     nothing wrong**, while thirteen other tests fail. It cannot see a
     total break of the machinery it is supposed to cover.
     The reason is worth keeping, because it generalises to every
     invariant of this shape. A differential invariant renders the same
     page two ways and compares — so it is blind to anything that moves
     both sides equally. It pays only where the two sides take *different
     paths*. The ones that have paid this session do: a mask sends a
     layer to a surface of its own, so masked and unmasked are different
     code; taking a blend off changes the surface decision, so blended and
     plain are different code. Grouping does not, and cannot, because
     source-over is associative — an isolated plain group and a flat run
     of layers are *proved* equal by the arithmetic, which is exactly why
     wrapping one changes nothing and exactly why the test can find
     nothing. The invariant is true, cheap, and worthless.
     So the question to ask of a proposed invariant before writing it is
     not "is it true" but "**do the two sides go down different code**".
     If the answer is no, it is a theorem about the arithmetic rather than
     a test of the renderer.
     The per-layer reading was then pointed at **pages nobody wrote**,
     since a random page is mostly small layers and the dilution there is
     worse than the fixture's. It does not transfer, and why it does not
     is worth more than the audit would have been.
     Sixty pages, forty-five of them drawn, 245 layers compared: two are
     flagged, and they are one thing — seed 31's `l4` and `l5`, a clip run,
     where hiding the base takes the layer held to it away as well. The
     first guess was the base's own antialiased edge multiplying into what
     it holds, and it is wrong: **31 of the 36 differing pixels sit where
     the base's coverage is full**, five at its edge.
     Bisecting `l5` names the ingredient exactly. As built, 36 pixels
     differ; with its stroke taken off, 4; with its gradient taken off,
     23; with its fill taken off and only the stroke left, 8; unclipped,
     46; with its smoothing off, 31. So it is the stroke — and it is the
     stroke *together with* the fill, since 36 is far more than 4 and 8
     apart. Giving the stroke the fill's own colour still leaves 20, which
     says the disagreement is about **coverage and not colour**: a shape's
     fill and its own stroke are two draws that share an edge, and the
     reference composites them one over the other while this backend
     resolves both out of one multisample buffer. Every difference found
     is under 0.25, which is one of that backend's four samples.
     A distinct mechanism from seed 2325's, which is worth saying because
     the two look alike from a distance: that one is `Effect::Outline` and
     a *threshold*, where a quarter-step of coverage flips a whole pixel
     of silhouette; this one is `Stroke` and a shared edge, with nothing
     thresholded anywhere. Same rule applies to both — **decline a wrong
     answer, tolerate a coarse one** — so nothing is declined and nothing
     is fixed.
     What that costs is the audit: a tolerance wide enough to let a
     shape's stroke through is wide enough to let most of a thin layer
     through, and a thin layer is all edge. The reading stays where it
     earns its keep, over the fixture, whose layers are large enough to
     have an inside. Written down so the next attempt at it starts from
     the measurement rather than from the idea.
     Six and a half, which is the same method pointed at a *function*
     rather than a constant: **weaken the whole of something and see who
     complains**. `apply_adjustment` was wrapped so that every adjustment
     came out at a fifth of its strength — the sense of every change
     unaltered, only its size — and then one variant at a time. Twelve of
     the thirteen were caught by a test named after them. The
     thirteenth, **vibrance**, was caught by exactly one test in the
     workspace: the GPU backend disagreeing with the CPU. That test
     self-skips where there is no adapter, so on a machine with no GPU the
     strength of vibrance was pinned by nothing at all, and the three
     things its own test asserted — a dull colour comes up, a vivid one
     moves less, grey stays grey — are all true of a vibrance five times
     too weak.
     It has anchors now, and two of the three are definitional rather
     than a regression pin, which is worth separating. At full chroma the
     weight is nought, so the colour must come back *exactly* as it went
     in: that is what makes this vibrance rather than saturation and it
     holds whatever weighting curve is used. Asked for nothing, likewise.
     The third is this definition's own arithmetic worked through by hand
     — sRGB (0.6,0.5,0.4) is linear (0.318547, 0.214041, 0.132868),
     saturation 0.582892, so the stretch about the luminance is 1.417108
     — and it is labelled a regression pin, because vibrance has no
     outside standard to be right against. What it buys is that changing
     the strength becomes a deliberate act rather than a drift. The
     fifth-strength mutant now fails in the render crate alone.
     The last optional oracle is the press profile, and the sweep there
     has a twist worth separating. A CMYK colour *through a profile* is
     inherently untestable without one — that is not a gap, it is what
     the thing is. What is not inherent is the **plain uncalibrated
     formula** every document uses until somebody loads a profile, which
     is the path most CMYK colours are actually seen through. What held
     it was black mapping to black — true of the formula with cyan and
     magenta traded, with any two channels traded, and with the inks read
     in any order at all, since at full black every channel is nought and
     the arrangement cannot be seen. Trading cyan and magenta did in fact
     survive the whole workspace with no profile set.
     The meaning is physical and each ink can be asked for its own: cyan
     absorbs red, magenta green, yellow blue. So a full cyan leaves green
     and blue and takes the red away; two inks together leave only the
     third light, which is what makes cyan over magenta blue rather than
     a muddier cyan; and a half ink moves its own channel and no other,
     which says the inks do not leak. Four mutations are caught in the
     colour crate alone with no profile anywhere.
     Worth keeping in mind for the next sweep: **a test that self-skips
     is not a test on a machine that skips it.** Twelve adjustments
     happened to have a second, local pin; one did not, and nothing said
     so until the mutant was tried with the GPU excluded.
     The same sweep over the six *filters*, run twice — once with the
     whole workspace and once with the GPU crate excluded — found two
     more of exactly that shape. A blur, a smear, a grid of squares and
     a vignette each keep a pin of their own. **Sharpen** and **noise**
     had one pin apiece and it was the GPU drawing the same page, so
     without an adapter either could run at a fifth strength unnoticed.
     Running the sweep the second way is what makes it say so, and it
     costs one more flag.
     Neither wanted a hand-computed number, which is the interesting
     part. Sharpen is an unsharp mask, and its strength cannot be written
     out without writing out the blur under it — three iterated box
     passes. It does not have to be: the blur is *measured*, by asking
     for it on its own page, and then the identity is exact — sharpened
     is the original plus the amount times what the blur took away. That
     pins the composition and the amount together and leaves the blur to
     its own test. Linearity in the amount would not have done, and it is
     worth knowing why: the overshoot at two is twice the overshoot at
     one whatever constant the whole is multiplied by, so a weakened
     sharpen satisfies it perfectly.
     The third sweep, over the sixteen **blend modes**, found the same
     shape again — *overlay* and *hue* survive weakening without the GPU,
     and six more are down to a single pin — and then a real defect
     behind it, because this time the fix was not two tests but an
     oracle. Blending has a published definition (W3C Compositing and
     Blending Level 1), so all sixteen are held to formulas written from
     the spec rather than from the code under test, over seven backdrop
     and source pairs including the corners. Soft light is the one where
     that is visibly not a copy: the spec's piecewise `D(cb)` is folded
     differently in the engine and the two agree anyway.
     Fifteen agreed. **Colour burn did not**, and it was not the blend's
     fault: the spec asks `is the backdrop one` before anything else, and
     white was arriving a rounding short of one. `1.055 × 1 − 0.055` is
     0.999_999_94 in f32, so `linear_to_srgb(1.0)` was not 1.0, the
     branch was missed, and the next one — `is the source nought` —
     answered instead. **White paper under a black layer set to colour
     burn came out black.** Fixed where it belongs, in the curve, since
     every reader that branches on the top of the range would otherwise
     have to know; the curve is now exact at both ends, still monotone,
     and unchanged in between.
     Two sweeps found nothing, and both are worth recording as negatives
     so the next search does not start there. The three **effects** — a
     drop shadow, an inner shadow, an outline — each keep pins of their
     own without the GPU. So do **text advances**: narrow every glyph's
     advance by a tenth and `the_shaper_and_the_rasterizer_agree_on_the_
     scale` fails, with no adapter needed. A sweep that comes back empty
     is the cheapest result there is and the only one that tells you
     where to stop.
     A fifth, over the **blur**, found the largest hole of the series.
     Its tests said a blur *blurs*: the peak flattens, neighbours light
     up, the total is conserved, nothing happens at zero, a flat field is
     untouched. All five hold for a blur at sixty per cent of the width
     asked for — and that mutation passes the whole workspace once the
     GPU crate is left out. The width of a blur, which is the only thing
     a blur really has, was pinned by a test that self-skips.
     What pins a width is the second moment: blur one lit pixel and the
     variance of what comes back is the square of the blur's effective
     standard deviation, which no amount of flattening gets right by
     accident. The number to hold it to is *not* sigma squared, and that
     is the part worth writing down rather than rediscovering. This is
     the W3C filter construction — three box passes of
     `floor(sigma × 3 × √(2π) / 4 + 0.5)`, about 1.88 sigma — and three
     boxes of width `w` have variance `3(w² − 1)/12`, so what it delivers
     is about `0.88 σ²`: a blur some six per cent narrower than its name,
     by design, because that formula matches an equivalent width rather
     than a variance. Matching it is the point, since an SVG saying
     `stdDeviation="4"` should soften here the way it softens in a
     browser.
     Small sigmas are left out, honestly: the box radius is an integer
     halved, so under about five the quantisation is coarser than the
     thing being measured — at sigma one the delivered variance is twice
     what the name suggests. Held to a tenth from five up, where a
     fifteen per cent error is caught, let alone sixty.
     Worth noticing how it was found. Nothing about that defect is
     visible from inside: the blend is right, the branch is right, the
     table is built right, and each of the three is checkable on its own
     without the bug appearing. It took comparing the whole against an
     outside definition at a corner nobody would think to probe by hand —
     which is method seven doing exactly what it is for, on a path that
     is not even one-way.
     Noise is random and has no value to assert, but it has a *range*:
     each speck shifts a cell by `(speck - 0.5) × amount` with speck in
     nought to one, so no pixel may move more than half the amount and,
     over nine hundred cells, one very nearly does. Both halves are
     needed and each catches what the other cannot — the ceiling alone
     passes for grain that does nothing, the floor alone for grain three
     times too strong, and both were tried.
     Seven, and new: **for a one-way path, borrow an oracle**. Almost
     every audit here leans on a round trip — write it and read it back,
     draw it both ways, apply it and undo it — and *importing* has no
     round trip to lean on. There is nothing to compare an imported file
     against except an idea of what it should have said, which is why
     four separate losses sat in `svg_import.rs` while every other edge
     of this program was watched: a picture dropped, a dash pattern
     written over, a fill rule never read, a clip path never read. The
     fix is not more care, it is a second implementation of the same
     spec. resvg is already a dev-dependency for the export witness;
     pointing it at the *import* side turns "does this look right" into
     "does this agree with a reader that is not us", and all four of
     those defects are now held that way.
     Where no second implementation exists, the *definition* will do.
     The ICC check on import said `assert_ne!` — that a tagged pixel
     came out different — which rules out the profile being ignored and
     nothing else: channels traded, the transfer applied twice, half a
     conversion would all have passed it, and one of those is a plausible
     mistake rather than a contrived one. Display P3 and sRGB share a
     white point and a transfer function and differ only in their
     primaries, so the answer follows from the two matrices: (200,100,50)
     in P3 is (215,93,31) in sRGB. The engine says (214,92,31), one level
     under on two channels, which is what a real profile's own chromatic
     adaptation costs against the arithmetic. It is asserted to two
     levels now, and the three mistakes above are each caught — the
     half-converted one especially, since that is the one an
     is-it-different check can never see.
     Eight, and it is a habit rather than a method: **the case has to
     break the tie**. Every test written for those four losses passed on
     its first draft *with the fix removed or inverted*, and always for
     the same reason — the example chosen could not tell the right answer
     from a plausible wrong one. A picture in a square box cannot tell
     the two scale axes apart (and an oblong box cannot either, since SVG
     letterboxes by default). A dashed line with no transform cannot tell
     whether the pattern was scaled. Rings all wound the same way cannot
     tell whether the winding sign was looked at. Clip outlines that do
     not overlap cannot tell a union from an intersection. In each case
     the fix was a second example chosen for that one purpose, and the
     way to find out is the same every time: break the thing the test
     was written for and watch it pass.
     Six, and new: **move a number and see who complains**. The vacuous
     miter check above was found by accident, which raised the obvious
     question — how many more are there? — and that question is
     mechanical. Take each tunable constant in `core/doc` and
     `core/render`, change it by enough to matter, run the workspace, put
     it back. Eight of them, a minute each. Five were caught at once
     (both marker numbers, the synthetic slant and weight, the tile
     size). Three survived, and the three are worth separating because
     only one of them was a gap.
     `MITER_LIMIT` at 2.5 instead of 4 survived, and that *was* a gap:
     the repaired check asks what happens to a corner past the limit, so
     loosening the limit fires it and tightening it does not, though a
     mitre lost is as wrong as a mitre kept. A corner of 35 degrees —
     between the limit as it stands and any plausible tightening — now
     says the near side: its point must reach past where a bevel would
     stop. That one mutant is caught now.
     `MAX_DEPTH` at 64 instead of 256 survived, and that is a **false
     positive of the method**, worth naming so the next sweep does not
     chase it. Its test builds `MAX_DEPTH` groups and expects the next to
     be refused, so it moves with the constant on purpose: what it pins
     is that the guard fires where the number says, and the number itself
     is policy rather than correctness. A mutant that survives because
     the test reads the same constant is a test doing its job. (The same
     shape caught me writing one: the new `stroke-miterlimit` assertion
     formats `MITER_LIMIT` into the string it looks for, so changing the
     constant cannot fail it — correctly, since the point is that the
     exporter says whatever the engine says. What makes it non-vacuous is
     that deleting the attribute fails it, and that is what was checked.)
     `WELD` survived at 1e-1, at 1.0 and at 5.0 — five thousand times
     looser, five whole document pixels, on a page where the shapes in
     those tests are ten across. It resisted an obvious test for a reason
     worth keeping: welding only matches *ends*, and the coordinates
     themselves are kept, so with two convex outlines there are two
     crossings, the pairing is unambiguous whatever the tolerance, and
     the answer comes out right however coarse the grid. A sliver three
     hundredths of a pixel wide survives a weld a hundred times too
     coarse. What it takes is an outline that crosses *itself*, so one
     small neighbourhood holds several fragment ends and the chain has a
     choice about which to join to which.
     That is now written, and it was found rather than invented: four
     hundred random self-crossing pairs through all three operations at
     two tolerances, and the signatures diffed. Six of twelve hundred
     answers move at a weld ten times looser. One of them — the case is
     kept verbatim, since rounding its numbers moves the crossings apart
     again — loses a piece of its intersection, the area dropping with a
     vertex, while its union and difference keep their area and lose
     vertices, which is the chain taking a short cut across a corner it
     no longer believes in. It fails at 1e-2, 1e-1 and 5e-1, and still
     passes at 1e-4, since welding *less* cannot merge what is distinct.
     Method three earning its keep on a question the other methods could
     not reach.
     Five, and new: **ask the document what it has never said**. The
     four above all start from a person deciding what to look at, which
     is the thing they have in common and the limit they share. This one
     does not: serialize the shared fixture, collect every key and every
     string value in it, and hold that against what the crate declares —
     every field of every struct, every variant of every enum. What comes
     back is the vocabulary the document owns and has never used, which
     is a list nobody has to think of. It costs one throwaway test and it
     is worth re-running whenever a kind of layer or a setting is added,
     since the gap it finds is the one that does not look like a gap:
     a field nobody set is obvious, and an enum whose fallback is a real
     answer is not. The first run said every field was covered and named
     `StrokeAlign` — three variants, none of them ever held — which is
     the fourteenth shape above. It also named the blend modes that are
     not separable, the adjustments beyond exposure and the filters
     beyond a blur, which are variations on arithmetic that already has
     audits of its own; the interesting ones are where a variant changes
     *geometry* rather than a number, and that is how to read the list.
     With one correction, learned from the shape that came out again: a
     variant that changes geometry is worth *looking at*, not worth
     adding on sight. The joins and the caps change geometry and the
     shared document still cannot hold them usefully, because no audit
     over it can see a corner. The method names candidates; whether one
     earns its place is still settled by breaking the code it was meant
     to exercise. Reading the list is worth doing anyway — the two
     defects above came out of looking at `StrokeJoin` and neither is
     in the fixture.
     Four, and new: **ask the stack rather than the command**, which
     found a real defect one floor up as well. A gesture — the drag API
     the whole editor's live editing runs through — kept the *first*
     preview's inverse and threw the rest away. That is exactly right
     for a drag, which restates the same command on every pointer
     sample, and wrong for a gesture that writes more than one thing: a
     gesture setting a layer's opacity and then its name undid the
     opacity and left the name where the gesture put it, committed or
     cancelled alike. Every gesture audit in the workspace previewed one
     command, so nothing saw it. A gesture keeps the inverses it needs
     now, applied in reverse so each lands in the state it was computed
     for, and drops a preview that writes only where the gesture has
     already recorded the way back — which is what a drag is after its
     first sample, so two hundred samples are still one command to undo
     and a brush stroke is two (`a_long_drag_is_still_one_inverse` says
     so as the shape of what history holds, because "one undo step" was
     true of the broken version too). Which slots a command writes is
     named rather than guessed, and the two sorts that cannot be named —
     the structural ones, and the ones that move the whole page, where a
     second resize offsets every layer again from where the first left
     it — record their inverse regardless
     (`a_gesture_of_more_than_one_kind_undoes_the_whole_of_itself`, a
     hundred and twenty gestures drawn from the same list of every
     command there is).
     And it was live rather than only latent, which was worth finding
     out: resizing a *frame* emits one command per pinned child and
     leaves out the children that do not move. Pull the south edge and a
     layer pinned to the east has nothing to do; carry on into the
     corner and the width changes and that layer moves — on the fifth
     pointer sample of a gesture whose first sample never mentioned it.
     Under the old rule the frame came back and that layer stayed where
     the drag left it. A test drives exactly that drag now and fails on
     demand against the old rule
     (`a_frame_dragged_into_its_corner_undoes_every_layer_it_moved`).
     The slot table itself is audited rather than read, because a wrong
     name there loses an edit in silence and the strings are plausible
     either way round: wherever one command's slots are covered by
     another's — exactly when a gesture drops the second inverse — both
     are applied and only the first's inverse undone, and the document
     has to be back
     (`a_commands_slots_name_what_it_writes_and_nothing_elses`). Give
     `SetName` the opacity slot and that fails while the hundred and
     twenty random gestures pass, which is what makes it worth having
     beside them. It wanted one thing in the shared list that was not
     there: a batch of nothing but field edits on more than one layer —
     what the app sends when several layers are picked and a slider
     moves — since every other batch there carries something structural. Undo is
     the invariant this whole editor rests on, and it was audited one
     command at a time against a document nothing had happened to yet.
     That is not what undo is: it is a stack, and what it has to survive
     is a command applied to a document five other commands have already
     changed, and somebody undoing three things, doing a fourth, and
     finding the redo branch gone. So every command there is, shuffled
     into a run and applied through the real `History`, then undone to
     the bottom and compared and redone to the top and compared
     (`a_run_of_commands_undoes_to_exactly_where_it_started`); and a
     random walk of apply/undo/redo, which reaches the thrown-away
     branch on its own, undone to the bottom and compared
     (`undo_and_redo_interleaved_come_back_to_where_it_started`). Both
     spot-check the narrowest claim as they go: one step back and
     forward again is the document unchanged.
     The two are stronger than the audit they sit beside, and it was
     worth proving rather than assuming. Both `MoveNode`s in the shared
     list crossed into another group, so reordering a layer *within* its
     own parent — what dragging a layer up or down the list does, much
     the commonest move there is — had never had its inverse checked at
     all. Two went in. Then the plausible mistake was made on purpose:
     the inverse index one too high in exactly the same-parent downward
     case. The one-command audit still passes, because the group holds
     two children and an index one too high clamps to the end, which is
     where it belonged. Under a run, earlier commands have put more
     children in that group, nothing clamps, and both new audits fail.
     Three, and by now not new: **pages nobody wrote**
     (`chitrakar_doc::fixture::page(seed)`). The fixture answers "does
     this hold for a document with one of everything in it", which is the
     question worth asking first and the one a person can keep in their
     head. It cannot answer "does this hold for the combinations nobody
     thought of", because every combination in it had to be thought of to
     be put there. A page drawn from a seed costs nothing to ask and
     there are as many as an audit wants. Two audits take them now: a
     hundred and twenty of them into a `.chitra` and back, held to the
     picture and not to the document (`a_page_nobody_wrote_goes_into_a_
     file_and_comes_back`), and the same hundred and twenty drawn both
     ways wherever the GPU backend takes them
     (`pages_nobody_wrote_are_drawn_the_way_the_cpu_draws_them`).
     The seed count is a dial, and turning it up is the cheapest search
     there is. At two thousand instead of a hundred and twenty the
     cross-renderer audit found **seed 557**, and behind it a defect in
     the *reference* renderer — which is not the way round this project's
     convention assumes, and is the reason it took a spec to settle.
     A shape with a fill and a stroke and a blend mode had the blend
     applied **twice** in the stroke band. Almost every layer here puts
     down more than one mark — a shape draws its fill and then its
     stroke, a text block draws a run at a time and its underlines
     besides, a brush layer draws every stroke — and a layer drawn
     straight onto the page took the blend once per mark. Where two of
     them overlapped, the backdrop was blended again: the stroke came
     down onto an already-blended fill instead of onto the page.
     Both halves were doing exactly what they were told, which is why
     nothing inside could see it. What settled it was arithmetic from
     outside: in the band the layer's own content is an opaque stroke
     over an opaque fill, so the answer is `Overlay(backdrop, stroke)` —
     the GPU's answer to three places — and blending twice gives
     `Overlay(Overlay(backdrop, fill), stroke)`, which was the CPU's to
     three places. The backend was right and the reference was wrong.
     A blended leaf composites as a unit now, on a surface of its own,
     exactly as the backend already had it and as this renderer already
     did for a group. Adjustments and filters are the same exception
     they are there: they rewrite what is under them, so a blend means
     nothing to them and a surface would only cost. It does cost, and the
     number is worth having: a *page-filling* blended rect went 68.2 ms
     to 75.6 ms, about a ninth, and a small blended shape pays a small
     surface since the surface is cut to the layer's own extent.
     The test says both answers, so a regression cannot pass by landing
     on the other one, and it checks the palette tells them apart.
     Turning the dial again, with that fix in, says two useful things.
     Seed 557 is clean, which is the fix holding. And two pages the
     earlier run never reached are not: the audit asserts per seed and so
     had stopped at 557. **Seed 1206**, at a mean of 0.0101 against a
     ceiling of 0.004, is a second defect in the reference renderer,
     and it took three attempts to fix, which is why the two that were
     withdrawn are written out below rather than quietly dropped.
     It reduces to three layers and a fourth that draws nothing. A page
     48×36; an opaque ground, sRGB (0.2122, 0.657, 0.7807); an ellipse
     `rx` 10, `ry` 7 filled sRGB (0.5422, 0.4635, 0.3696) at α 0.45862
     with blend **Lighten**; a copy of that ellipse elsewhere; and above
     the copy a layer *held to* it — hidden, since what it draws is
     beside the point and the clip run is the point. Take the held layer
     away and the two renderers agree.
     The arithmetic names the right answer without asking either of them.
     Lighten is `max` per channel, so over an opaque backdrop the answer
     is `as·max(Cb,Cs) + (1−as)·Cb`; the colours are chosen so that green
     and blue have the source *darker* than the ground, which is the only
     place Lighten and Normal differ at all. Green should be 0.38918 and
     the reference renderer gives 0.29407, which is `as·Cs + (1−as)·Cb`
     exactly — plain Normal. The backend gives the spec's answer. **A
     copy of a layer with a blend mode loses the blend.**
     The mechanism is worth having, because it is not a missing branch.
     A copy goes onto a surface of its own for reasons that have nothing
     to do with its colour: it is faded, it is masked, or — as here — the
     layer above is held to it, so its own alpha is wanted back and a
     surface is where that is read. That surface starts empty, and every
     separable blend collapses to Normal against nothing, `ab` being zero
     so the blended term drops out of the compositing formula. Which is
     exactly right for getting the layer *into* the surface, and exactly
     why the blend has still to happen when the surface comes down. It
     comes down by `node.blend` — the *copy's* own, `Normal`. So the
     blend is not mishandled anywhere; it is spent against nothing and
     then never asked for again.
     Two fixes were written and both were withdrawn, and the measuring is
     the part worth keeping. The first hoisted the copied layer's blend
     onto the copy (`meeting_blend`) and used it for the *surface
     decision* as well as the composite, so a copy of a blended layer is
     isolated the way a blended leaf now is. The whole workspace passed —
     466 tests, the cross-renderer audit among them — and the two
     thousand seeds said **ten pages worse**, against the two it fixed.
     The reason is already written down two paragraphs up, about effects:
     a copy's extent is not its content's, what it draws being another
     layer somewhere else, so a surface cut to the copy's box cuts the
     picture. That is why an effect cannot hang on a copy either, and it
     was there to be read rather than discovered.
     The second kept the straight path — where a copy is already correct,
     the layer it copies being drawn by its own `draw_layer` and bringing
     its blend with it — and hoisted the blend only where a surface had
     been taken anyway. Five pages worse. Seed 860 says why, and it is a
     shape nobody would think of: a copy of a copy, where the inner one
     carries Multiply, and the outer one also wears an *effect*. An
     effect comes down by the layer's blend as well as the layer, so the
     layer would arrive by Multiply and its shadow by Normal — two
     different blends for one picture.
     What worked is narrower than either, and the narrowness is the
     point: a surface taken *only* to hand the layers above their base's
     alpha should not change the picture at all, so for a copy it is not
     taken. The copy goes down straight, where its blend meets the real
     page, and the alpha those layers want is drawn again aside. A copy
     alone — a group on a surface is *isolated* by it, which is the whole
     reason it is there, and a leaf's own blend is applied when its own
     surface comes down and so was never lost. Being read as the cut is
     the weaker of the two things a surface is for, and for a copy it is
     too weak to pay for.
     It costs a second drawing of the copy, and only for a copy that
     something is held to. Which is the trade the other two attempts were
     trying to avoid and should not have been: the reference renderer is
     the thing every other answer here is measured against, and a surface
     it cannot justify is worse than a page it draws twice.
     Six thousand seeds say it is right rather than merely better. Over
     the two thousand, nothing at all is above the mean now — 1206 and
     1389 both clean, 1392 pages drawn, and the worst pixel anywhere down
     from 0.885 to 0.607. The lesson from the two withdrawals is that the
     workspace passing is not evidence: both of them passed all 466 tests,
     the cross-renderer audit among them, and only the dial could tell.
     And the dial, turned to six thousand, hands over the next seven
     (2325, 2854, 3091, 3775, 4162, 4898, 5985). They are not regressions
     — every one gives the same number with this fix switched off, which
     is worth checking before chasing any of them — so they are new
     ground rather than damage. Seed 4898 was the one to take first, at a
     mean of 0.240 against a ceiling of 0.004 — sixty times over, where
     everything found so far had been two or three times over — and it
     paid, though not for the reason it looked like.
     It presents as a copy of a copy, and it is not about copies at all.
     Undressing it said four things were needed at once: a mask on the
     inner copy, a mask on the outer one, a blend, and a drop shadow.
     Then the copies were replaced by a **group** holding the same masked
     shape, dressed the same way — and the check that makes that worth
     anything is that the undressed pair agree *exactly*, on both
     renderers, so the two constructions really are the same page and a
     placement mistake cannot be read as a defect. Dressed, each renderer
     draws the copy and the group identically to itself and the two
     disagree with each other by a third of a channel. So the copy was
     scenery.
     Sixteen combinations of the four, on the group alone, name it: **a
     mask inside a masked layer that casts a shadow**. The blend is not
     needed — without it the disagreement is worse, 0.0204 and 0.681 —
     and either mask alone is clean to a ten-thousandth.
     And the arithmetic is not needed either, because a sharper oracle
     turned up: this backend's answer for that page is the *reference
     renderer's answer for the same page with the child's mask removed*,
     to 0.0002. Not a softer shadow, not a shifted one — the child's mask
     dropped altogether. One coverage texture is what a pass here has, an
     effect is built from the layer's silhouette, the layer's own mask is
     what decides that silhouette and so rides the slot, and a mask
     inside wants the same slot and does not get it.
     So the page goes back, which is what a stroke carrying a region
     already gets on a layer whose own mask is on that slot, and this is
     the row beside it. Teaching the pass a second coverage is the real
     answer and is not small; declining is honest and is available now.
     Three of the seven go with it (3775, 4898, 5985), 1780 pages declined
     instead of 1772. The test asserts the refusal, asserts that either
     mask *alone* is still drawn and drawn the reference's way so the
     limit is no wider than the collision, and asserts that the two
     pictures really differ so what is declined is a wrong answer rather
     than a scruple — and it was checked both ways round, since a refusal
     made too wide passes the first of those three perfectly well
     (`a_mask_inside_a_masked_layer_with_effects_goes_back`).
     What is left is four (2325, 2854, 3091, 4162), and the lesson to
     carry into them is this one's: the shape a rough page *presents* need
     not be the shape of what is wrong with it, and replacing the exotic
     part with something ordinary — a group where a copy stood — is the
     cheapest way to find that out. The guard is to check that the
     ordinary version agrees before it is dressed.
     **Seed 2325 is diagnosed and deliberately not fixed**, which is a
     different answer from 4898's and the difference is the point. It is a
     three-point closed **path** with an *outline*, and taking the outline
     off is the only thing that settles it. An outline's band is a
     distance from a *thresholded* silhouette — half of the layer's own
     opacity is inside, which is what lets a faded layer have an inside at
     all — and this backend antialiases a path by four samples a pixel. So
     a quarter-step of coverage flips a whole pixel across the threshold,
     and a whole pixel of silhouette moves a two-and-a-half-wide band
     visibly. Measured on plain pages: a rect with an outline is exact to
     0.00007 at every fill alpha, because a rect's coverage is exact on
     both sides; a path with an outline is 0.0012 to 0.0023 at *every*
     alpha above the threshold, and clean below it where there is no
     inside and so no outline.
     A second and quite separate instability came out of the same sweep,
     and it is nobody's defect: a **disc** with an outline degrades as the
     fill alpha approaches the threshold — 0.00011 at 0.80, 0.00023 at
     0.60, 0.00065 at 0.52, 0.00155 at 0.504. Thresholding is
     discontinuous by definition, so two rasterizers differing by a
     thousandth of coverage differ by a whole outline. The seed's fill
     alpha is 0.504. Worth knowing before reading any outline comparison.
     Not declined, and this is the rule the two findings together give:
     **decline a wrong answer, tolerate a coarse one.** The mask collision
     above made the backend draw a *different picture* — a mask dropped —
     and there is no defending that. Here it draws the same picture
     measured more coarsely, which is the path-antialiasing gap already
     written down two paragraphs up as the biggest of the three and not
     small to close. Refusing every path that carries an outline would
     hide a coarseness and shrink what the audit compares, which is the
     wrong trade.
     **Seeds 2854 and 4162 were one defect**, and the third in the
     reference renderer this stretch. What found it was not the backend at
     all but a question the renderer can be asked about *itself*: **a blend
     decides how a layer meets what is under it, never what it covers.**
     That follows from what a blend is, so drawing a page twice — once with
     a layer's blend and once with it set to Normal — must ink exactly the
     same pixels. It did not: 97 of them went missing.
     A blend puts a layer on a surface of its own, and a surface has to be
     cut to something. For a copy that something was a box meaning
     *nothing at all*. A copy of a group holding an **adjustment** has no
     finite box — an adjustment reaches as far as what it changes — and
     `local_bounds_of` can only answer `Option<[f32; 4]>`, so "everywhere"
     came back as `None`, and `bounds_in_parent_space` turned that into
     `Bounds::None`, which is the box of a layer that draws nothing. The
     surface was cut away and the copy lost the part of itself outside it.
     The two readings of the same question were already in the file, four
     hundred lines apart: `render_child` has said `None => Bounds::Everything`
     for as long as it has drawn a copy, with the reason written beside it,
     and the extent that cuts the surface said `Bounds::None`. So the fix is
     to distinguish the two things `None` was carrying — the original *gone*,
     which really is nothing, from the original having no box because it
     reaches everywhere.
     It clears 2854 and 4162 both, 4162 being a *masked* copy rather than a
     blended one and so taking a surface for the same reason by another
     road, which is what says the fix is about the box and not about blends.
     Four thousand two hundred and twenty pages drawn of six thousand and
     two left over the mean: 2325, which is the path-and-outline coarseness
     above and is staying, and 3091 at 0.0041, a hair over.
     Two things about the finding worth more than the fix. The first is the
     oracle: it needs no second renderer, no spec and no arithmetic — the
     page is drawn twice and compared with itself, and it would have found
     this on a machine with no GPU at all. The second is that the layer
     that breaks it is **hidden**, exactly as the seed has it, and the test
     keeps it hidden on purpose: a hidden layer paints nothing, so it can
     only be reached through a question about *extent*, which is what makes
     it proof that the box is wrong rather than the drawing
     (`a_blend_does_not_decide_what_a_layer_covers`; take the fix out and it
     loses 101 of 202 pixels drawn, half the layer).
     With the four fixed, the dial **stays where it paid**: the seeded
     audit draws two thousand pages now rather than a hundred and twenty,
     and passes, at about half a minute. Nothing in 0..2000 is over the
     page mean any more — the two that are, 2325 and 3091, lie past it —
     so turning it up cost nothing but time, and leaving it down would
     have meant throwing away the instrument that found all four.
     That audit has a second reading now, on the argument the export
     witnesses were rebuilt over: **a page-wide mean is a poor thing to
     hold two rasterizers to**, because every edge costs it a little, so
     it rises as a page gains elements and the ceiling has to be loosened
     to let innocent additions through. What is *not* allowed to differ is
     the inside of a shape — a pixel whose eight neighbours the reference
     renderer draws in its own colour, so neither side has an edge to
     disagree about there. That leaves out exactly what is allowed to
     differ, and grows rather than thins as a page does.
     It earns its place on evidence rather than on the argument. Put the
     copy-bounds defect back and the interiors fail at **seed 679** — a
     page the mean passes, and two thousand seeds earlier than the one
     that originally found it. Put the mask collision back and they name
     786, 33 and 7 points on their pages. Meanwhile the two pages the mean
     still calls rough do not move the interiors at all, which is the
     whole point: the reading that means *correct* is silent on coarseness
     and loud on defects.
     Two numbers, because one is not enough. A point ceiling of a fifth of
     full scale, and an interior *mean* of 0.003 — the fifth defect page (a
     copy of a group holding text, where what was lost is glyph-sized and
     so nearly all edge) raises that mean fiftyfold, 0.00017 to 0.00891,
     without a single point crossing the point ceiling at all. The other
     way round happens too: seed 679 has three points over and an interior
     mean of 0.00191, under the ceiling. Neither reading subsumes the
     other, which is why both are taken.
     The point ceiling is a fifth and not a hundredth because of the
     backend's own resolution: four samples a pixel, so a sub-pixel crack
     between two tessellated pieces costs a quarter of one sample, which
     against a strong colour is a quarter of full scale. **Seed 283 is
     exactly that**, at 0.164 — one pixel of a stroke drawn at three
     quarters where the reference draws it whole, on a smooth path with a
     band 2.8 wide, page mean 0.0005. Under the ceiling is the backend
     being coarse; over it, something is drawn wrongly. The crack was
     chased and not found, and the false trails are worth marking: it is
     not the disc tessellation (rounding those polygons *out* to the
     circle rather than inscribing them changes the number not at all),
     and it is not parity in the stencil (a stroke's pieces go through a
     union pipeline rather than the fill's `Invert`).
     And the two old numbers were **re-based, not loosened**, a
     distinction worth insisting on since a quietly loosened ceiling is
     how an audit stops catching things. Both were counts over a hundred
     and twenty pages, and a count is a fact about how many pages were
     looked at rather than about the renderers. "How many are rough" is a
     proportion now — 24% of those drawn, against 29% at a hundred and
     twenty, so it went *down* — and "the worst pixel anywhere" is a
     maximum over sixteen times as many samples, so 0.37 became 0.65.
     Neither renderer got worse. What makes that safe is that the claim
     those two used to carry has moved to the interiors, where it is
     stated per point and does not drift with the page count at all.
     One thing the seeded audit does **not** cover, said plainly rather
     than assumed: no page in 0..2000 exhibits the mask collision, so
     putting that refusal back leaves the audit green. Its own test is what
     holds it, and that one was checked both ways round.
     And **seed 1589** is the reason both readings measure a channel
     against its own size once it is over one, which is worth more than it
     looks. It was the worst interior mean of two thousand pages at
     0.00496, and it is not a defect: light is not bounded here — two stops
     of exposure put a channel at three — and the backend keeps its surface
     in `Rgba16Float`, whose steps in [2, 4) are about a five-hundredth.
     Its worst point is 3.0010 against 2.9961, two of those steps and a
     sixth of a per cent, accumulated over two adjustment passes. Taken
     absolutely, a bright page reads the *storage* rather than the drawing.
     With the correction the worst mean anywhere is 0.00214 rather than
     0.00496, and the ceiling is 0.003 rather than 0.006 — the same
     evidence, read for what it says. Which is the lesson of the whole
     stretch in one line: a number is worth only as much as knowing what it
     measures.
     And then a fourth defect in the reference renderer, found by an
     invariant rather than by looking, and by one that needs no second
     renderer at all: **the alpha a page comes out with cannot depend on
     any blend mode in it.** Compositing alpha is `as + ab(1 - as)`
     whatever the blend function does to colour — `separable` says exactly
     that — so taking every blend off a page must leave its coverage
     untouched. Seventeen of two thousand pages nobody wrote changed by
     more than a hundredth, seed 295 by 0.87 over four hundred pixels.
     Every one of the large ones was **a copy of a filter wearing a
     blend**. A filter is not a picture laid over the page, it is a change
     to what is under it, so a blend on one means nothing — asked
     directly, Darken, Multiply, Lighten and Difference each change not a
     pixel. Both renderers knew that and both asked it the wrong way:
     `matches!(node.kind, Adjustment | Filter)` answers what a layer *is*,
     and a copy is an `Instance`. So the blend put the copy on a surface
     of its own, what it copies was handed a transparent page to filter,
     and it came back with nothing — the layer vanishing because it was
     blended. Both sides walk what a layer *draws* now, through
     `rewrites_what_is_under_it`, which the backend borrows from the
     renderer so the two cannot drift apart on it.
     Two things worth keeping. The first is that the two sides **agreed
     while both were wrong**: fixing the reference alone made the
     cross-renderer audit fail at seed 295, which is the right way round
     for that audit to behave and a reminder that agreement is not
     correctness — only an outside claim can say which of two agreeing
     answers is right, and here it was arithmetic about alpha.
     The second is the residue, recorded so the next search does not start
     there: nine pages still move their alpha, all of them small — two to
     eleven pixels, worst 0.084 — where the large family was 0.16 to 0.87.
     It is why the invariant is not yet a test over the corpus. It took
     three readings to describe correctly, which is worth recording as
     plainly as the answer: first "a fringe pixel lost to a surface's
     edge", then "the surface covers more and which is right is open",
     and only then the measurement that settles it. Each guess was
     plausible and two were wrong.
     What it is: **a layer that carries both a mask and a blend does not
     have its mask applied at the pixels on the boundary of its own box.**
     Every moving pixel sits on that fractional boundary — seed 413's box
     starts at (14.369, 8.152) and its pixels are (14, 8), (15, 8),
     (14, 9) — and the blended page *gains* alpha there, which is the
     mask's contribution going missing rather than the layer being cut
     short.
     Four renders of seed 346 say it without any argument. At (21, 21):
     blended 0.4621, plain 0.4231, plain with the mask taken off 0.4621,
     blended with the mask taken off 0.4621. So the mask lowers the alpha
     in the plain path and does nothing at all in the blended one, and
     0.4621 is exactly the area the picture's rectangle covers of that
     pixel, computed from its placement. The plain path is the right one.
     It wants a mask *and* a blend *and* a part-covered edge, which is why
     it is nine pages of two thousand and never more than eleven pixels;
     it is not raster-only (seeds 398 and 413 are vectors).
     It reproduces by hand now, which is the part that moves this on: a
     rect at (21.2168, 21.41), a vector mask whose edge falls inside the
     layer's own first column, and a blend. **The fourth ingredient is a
     `feather`.** At feather zero the two paths agree exactly (0.3133 and
     0.3133); at any feather at all they do not (0.3713 against 0.3829).
     `invert` only decides the direction — with it the blended page loses
     where without it the blended page gains — which is why the first
     measurements read as "gains" and the seed that prompted them happens
     to carry `invert: true`.
     Three mechanisms were proposed and all three were measured and
     ruled out, which is worth recording so they are not proposed again:
     it is **not** the surface being too small for the softening (growing
     `pad` by the feather's own reach changes nothing); it is **not** the
     blur, since feathers of 0.0398, 0.3 and 1.0 give *identical* numbers
     where their box radii differ, and only 3.0 moves them; and it is not
     the mask being dropped wholesale, since the mask plainly applies in
     both paths and only differs.
     Printing the plane's clip and its values on both paths settled it,
     and the answer was the fourth guess. A mask with a soft edge is
     worked out as a plane and then *blurred*, and a blur reads its
     neighbours. The plane is grown from the region being drawn and then
     held to the surface it is drawn on — `(0, 0, 48, 36)` for the page,
     `(0, 0, 13, 11)` for the layer's own surface — so at the plane's
     first row and column the blur has nothing to read and clamps. That
     is the layer's own edge, which is where its mask matters most: the
     same page pixel came out 0.4741 one way and 0.4889 the other.
     The `MaskPlane::at` smell above was a red herring and is worth
     striking rather than leaving: both planes cover their whole surface,
     so the 1.0 fallback never fires here. So was "not the blur" — the
     identical numbers at feathers 0.0398, 0.3 and 1.0 come from the box
     radius landing on the same integer, not from the blur doing nothing.
     Four mechanisms proposed, three measured wrong, and each one only
     fell to an instrument rather than to more reading.
     The fix is to grow the **extent** the surface is cut to by what the
     softening reaches. Growing the *clip* does nothing, which is the part
     that cost an hour: the surface is cut to the extent and the clip only
     narrows it further, so `pad` was the wrong dial and changing it moved
     not a pixel.
     With that, the invariant is a test rather than a note
     (`a_blend_never_changes_what_a_page_covers`, two thousand pages, about
     eight seconds): zero pages move their alpha by a hundredth where
     seventeen did, and three move by a thousandth, worst 0.0095. It is
     asked with the ground *hidden*, which is the whole reason it works —
     an opaque backdrop makes a page's alpha one everywhere and hides
     exactly what is being looked for. And it is asked for non-vacuity
     twice: that the pages carry blends at all, and that they draw
     anything once the ground is gone.
     The named case beside it is
     `a_feathered_mask_softens_the_same_on_a_surface`, which needs the
     mask's edge to fall inside the layer's own first column or both paths
     agree and it proves nothing — at `feather: 0.0` they agree to the last
     bit, which is what says the feather is the ingredient rather than the
     mask.
     Ten, and it is the same method pointed at the thing the editor
     actually does every keystroke: **a rectangle repainted has to be the
     page drawn whole.** Nothing redraws the whole page for an edit — the
     engine works out what a command touched and paints that much again
     over what is on screen — so if a dirty rectangle does not come back
     to what was there, the screen keeps pixels nobody could find by
     reading the document: a stale seam along the edge of whatever was
     last touched. It follows from what a dirty region *is*, so it needs
     no second renderer, and it had never been asked over pages nobody
     wrote (`a_rectangle_repainted_is_the_page_drawn_whole`).
     Forty-eight rectangles of two thousand pages failed it, and every one
     of them was the **feathered mask** again — take feathered masks out
     and it was clean, which is what said the seam and the surface were
     one thing. The plane a softening runs on is held to whatever is being
     drawn on, and there were three such places, each of which had to be
     given room before the next became visible: the layer's own surface
     (48 → 19 once the clip was grown as well as the extent), the surface
     a *group* or a *copy* is drawn on (19 → 8), and the margin inside
     `plane_over` itself.
     That last one is worth stating on its own, because it is a plain
     arithmetic mistake rather than a missing case: the margin was
     `3 * sigma + 1`, and the blur it is meant to cover is three box
     passes whose radius is the W3C width **halved by an integer division
     and then held at one**. At sigma 0.04 that reaches three pixels where
     the margin allowed two. It is `blur::plane_reach` now, taken from the
     radius the blur will actually use, so the two cannot drift apart.
     Each of the three is needed and the test names which: take the clip
     growth out and seed 136 fails, the two surface arms and seed 62, the
     margin and seed 1221.
     There was a fourth place, and it is closed too: a **clipped** layer
     carrying a feathered mask. What such a layer is held to comes from
     the base drawn aside, and *how far past the rectangle that alpha is
     wanted* counted the effects of the layers about to be cut by it and
     not their masks — `Effect::reach` and nothing else. A soft edge reads
     beyond its own pixels for the same reason a shadow does. With that,
     nothing of two thousand pages fails; take it out and seed 724 does.
     Four places, and the shape of the search is worth keeping: each one
     was invisible until the one before it was fixed, because any of them
     alone was enough to keep the rectangles wrong. A count that goes 48,
     19, 8, 0 is not four guesses, it is one cause found in the four
     places it had been written out.
     The same method, asked three more ways, and the third was the best
     yet. **A layer at no opacity draws what a hidden one draws** — clean,
     a negative worth recording. **A layer hidden draws what a layer taken
     away draws** — ninety-four pages of six hundred fail it, and the
     *claim* is wrong rather than the code: a run of clipped layers is
     formed from the flags without consulting visibility, so hiding a clip
     base keeps it the base while removing it promotes the next layer up.
     Worth writing down, because it is the one of the three that looks
     most obviously true.
     **A mask that hides nothing is no mask**, though, is exactly true,
     and it found a defect a user would have hit: **a copy of an
     adjustment vanished the moment it wore anything.** Masked by a mask
     that hides nothing it drew, to the last bit, what hiding it drew; at
     opacity 0.999 likewise; at a half likewise. The layer it copies has
     no such trouble, because an adjustment takes its mask inside its own
     pass — which is what says the fault is the copy's.
     The reason is the one from two commits ago, reached by the other
     door. An adjustment is a change to what is under it and cannot go on
     a surface of its own, there being nothing under a fresh surface to
     change; a *blend* was one way to send a copy to a surface and was
     fixed; a **mask or a fade** is the other and was not. So the copy's
     mask and opacity go down as a `Cover` now — the same thing
     `draw_layer` already hands an adjustment held to the layer below:
     render where it stands, then mix back by how much of the region is
     let through.
     And the backend had it too, which the cross-renderer audit said at
     once: fixing the reference alone made seed 665 fail. Its own masks
     ride the single coverage slot a layer already uses, so a coverage
     handed down is a pass it has no shape for — the page goes back, which
     is the answer a mask inside a masked layer with an effect already
     gets. Sixteen more pages declined of two thousand, 1374 drawn.
     What is left is a **second seam, wider than it first looked**, and
     the way it was nearly missed is the lesson. The sweep that found the
     adjustment defect only masked layers that nothing copied, and only
     two a page. Masking *every* leaf instead says six pages of six
     hundred still change when a mask that hides nothing goes on — and
     they are copies of a **raster, a vector and another copy**, not of
     adjustments at all. Worst 0.2955 on a single pixel, 0.1060 over
     eighty-seven, 0.1792 over twelve hundred. A copy drawn on a surface
     of its own is not the same picture as the same copy drawn straight,
     and a mask is what sends it to one.
     **That seam is now closed, and all six were one thing.** Every one of
     the six is a copy whose *target* wears a non-Normal blend — Overlay,
     Multiply, Darken, Difference — while the copy itself is plain. The
     blend belongs to what the copy draws, and a mask sends the copy to a
     surface where that blend meets a transparent page: every separable
     blend collapses to Normal against nothing, `ab` being zero, so it is
     spent and never asked for again. `copies_a_blend` is the fix, and it
     reuses the pass the adjustment case already built — the copy's mask
     and opacity go down as a `Cover` and what it copies is drawn where it
     stands, so the blend meets the page really under it. Six of six
     hundred to none. The backend refuses such a page on the same terms as
     the adjustment one, and for the same reason: a coverage handed down
     is a pass it has no shape for.
     Two conditions on it, and **both were bought with a failing test
     rather than reasoned out in advance** — which is the whole argument
     for keeping the coverage invariant next to the mask one:
     - *The copy must wear no blend of its own.* Seed 114 said so: a copy
       of a blended brush layer, itself Overlay at 0.59, had already been
       given a surface by `draw_layer` for its own blend, and drawing what
       it copies straight into that surface put the copied blend against a
       transparent page exactly as before, while losing the coverage the
       surface gets right. So such a copy keeps its own blend and spends
       what it copies. That is a real limit, and it is written down rather
       than papered over.
     - *What it copies must carry no effects.* Seed 569 said so, by half
       an alpha: a layer's own coverage is taken **before** its effects are
       made, so that they grow from the shape that will really be seen — but
       a copy's mask is not the copied layer's coverage. It belongs over the
       finished copy, the shadow it casts included. Handing it down cuts the
       layer before the shadow grows from it, which is a different picture.
     Both were caught by `a_blend_never_changes_what_a_page_covers`, not by
     the mask invariant that motivated the fix: the mask test says the
     colour is right and the coverage test says nothing was gained by
     moving pixels. Neither alone would have been enough.
     The draft of the mask invariant is still worth recording as a
     warning, because it is committed now only after being rewritten:
     written with the narrower sweep's filter,
     `a_mask_that_hides_nothing_is_no_mask` **passed with both of the fixes
     it was written for taken out**. Two layers a page, lowest-numbered
     first, and a copy is rarely the lowest — so it masked the ground and
     its neighbour and asked nothing. It bites at once when it masks every
     leaf, which is what it does now; its floors are the honest measured
     counts (2635 layers masked over 600 pages, 211 of them copies) rather
     than round numbers, so it cannot go quiet again without saying so.
     One change was written for the chain case and **withdrawn for want of
     evidence**, which is worth saying plainly: making `draw_layer` ask
     `rewrites_what_is_under_it` rather than `matches!(node.kind, ...)`
     looks like the obvious generalisation, reads well, and moves not one
     page of six hundred either way. It was kept only as long as the
     narrow sweep seemed to say 2 → 0, and that 2 → 0 was the filter
     rather than the fix. Three times today a plausible change to this
     renderer has had to be measured before it could be believed, and
     twice the measurement said no.
     And the leaf-picking in that sweep had to be sorted before any of it
     could be trusted — the same `HashMap` order that cost a wrong reading
     of seed 2854 gave a different set of failures on each run until it
     was. Twice now.
     The exclusions are wide for an honest reason and the test says so:
     an adjustment, a filter and a clone read what is under them, and an
     effect reads past its layer's own silhouette, so the engine grows the
     region or abandons it rather than repainting exactly that much. About
     one page in seven is left — three hundred pages and fifteen hundred
     rectangles, which the test also asserts, since every exclusion is a
     way for it to pass by asking nothing. The hand-built case is
     (`a_copy_of_a_filter_draws_the_same_whatever_blend_it_wears`), and it
     asks the tie as well as the claim: the copy has to *change* the page,
     or "the same whatever blend it wears" is true of a layer that draws
     nothing, which is precisely the broken answer.
     And one thing about the *instrument* rather than the findings, which
     cost a wrong conclusion before it was noticed: **the greedy minimiser
     is not reproducible.** Hiding every layer that is not needed to keep a
     page rough walks `Document::nodes()`, and that is a `HashMap` reseeded
     every run, so the order differs and so does the answer — seed 2854
     minimised to 0.02266 twice and 0.04061 once. Two undressings of it
     were compared across runs and were therefore comparing two different
     pages. Sort the ids. The 4898 conclusions do not rest on it: every one
     of them was reproduced on hand-built pages, which is why the sixteen
     combinations and the mask-removed oracle were worth the trouble.
     The first run found four things, three of them in the backend and
     one in the reference renderer.
     A copy *held to the layer under it* was drawn whole: a copy's draws
     come from walking what it copies, and that walk ended the one that
     would have held it back — its own mask was already a reason to hand
     the page over and being clipped was not. A copy *of* a layer that is
     held to the one under it was held back by that layer, where the CPU
     renderer draws it whole: what a copy draws is the layer, not the
     layer's place in a run of clipped ones. Both hand the page over now.
     A sharp corner was rounded off: the backend measures a shape by its
     distance, and a distance is the same in every direction, so the
     pixel diagonally outside a square's corner came out a tenth covered
     where the area it really covers is none. A rectangle with square
     corners is measured edge by edge now — two half-planes multiplied,
     which is the exact area for one standing square on the page and much
     the nearer answer for a turned one.
     And the reference renderer was the coarser of the two in one place:
     a rect's stroke *band* was sampled sixteen to a pixel, so its edges
     came out in quarters — a hairline eight tenths of a pixel over a
     boundary was drawn as three quarters of it. The band is one
     rectangle less another, and each of those is the same product of two
     1-D overlaps the fill already had exact, so it is exact now too.
     The pages that stayed rough were then run down, and all of it is a
     curve drawn two ways. Eleven of the nineteen hold a *path*, which
     the backend fills through a stencil and antialiases by multisampling
     it — four samples a pixel, which is the only count WebGPU makes every
     adapter offer and the only one this one accepts, eight and sixteen
     being refused outright. So a path's edge comes out in quarters there
     where the renderer being matched is exact across a row and sixteen
     deep down it, and that is the biggest of the three. (An earlier note
     here said the backend did not antialias a path at all, which was
     wrong: `SAMPLES` has been 4 since the stencil was written. Closing
     the gap means supersampling the whole page or handing the backend the
     other's answer, and neither is small.) Several hold a raster, and the
     note here used to say that was two samplers clamping their own way
     either side of the last texel. It was not, and the only reason to
     think so was that it sounded like a sampler problem: measured, a
     two-by-two picture set at (8.4, 6.3) read 0, a quarter, a quarter
     across its border row where the reference read 0.18, 0.30, 0.12 —
     which are exactly the areas, and 0/¼/¼ is exactly what four samples
     can say. The same quantization as the paths, on the quad's own edge.
     Unlike the paths it *is* closable, because a picture's border is a
     box and a box's coverage is a product of two 1-D overlaps: the quad
     is drawn a device pixel wider on every side now and the fragment
     shader fades it by that product, so the border is continuous and
     exact for a picture square to the page. Eight of the rough pages
     came clean with it — forty-two down to thirty-four — which is what
     says the cause was really that, and the test asks the border for
     the area arithmetic says it covers rather than merely for agreement
     (`a_pictures_border_is_the_area_it_covers`, eighty-eight part-covered
     pixels checked one by one; take the skirt off and the border reads
     0.21 where it covers 0.42, take the fade off and it reads 1.0).
     A shape's *fill* was a third cause and is not any more, and neither
     of the two things wrong with it was what it looked like. `fwidth` is
     |dx| + |dy|, where the rate a quantity changes across a pixel is the
     length of its gradient — the same number only when one partial is
     zero, and root two out on an edge at forty-five degrees, so every
     ramp scaled by it came out that much too wide. And an ellipse's
     first-order distance is singular at its middle, where the gradient
     goes to nothing: a number that large has no meaningful rate of
     change across a pixel, and it left a stray pixel three-quarters
     covered in the middle of a solid disc. That second one was the worst
     disagreement anywhere between the two renderers and it was not on an
     edge, which is why every way of looking for it as an edge problem
     had missed it — it was found by measuring one plain shape at a time
     against the reference and printing where the worst pixel fell, which
     is a cheaper instrument than the random pages and should have been
     reached for first. A disc's worst pixel went from 0.308 to 0.046, a
     squashed one's from 0.313 to 0.072, a rounded rectangle's corners
     from 0.148 to 0.062; a square-cornered fill was already exact and
     stayed so. Nine of the rough pages came clean
     (`a_shapes_edge_is_the_area_it_covers`, which asks each shape for
     the area arithmetic says it covers and for its total area besides,
     since a shape drawn systematically fat would pass the first alone).
     A shape's *band* went the same way next. A stroke is one outline
     less another, so measuring it by a distance is two roundings-off
     rather than one, and a square-cornered rectangle whose fill was
     already exact had the worst band of any shape at 0.195. It need not
     have: the outline grown and the outline shrunk are both boxes, the
     shrunk one lies inside the grown one, and a box's coverage is
     exact, so the area between them is the difference of two exact
     answers. 0.025 now, at every width and alignment tried.
     With one reservation, and finding it was the useful part: squaring
     the corner off made an outside-aligned stroke *much* worse — two
     thirds of full scale, against the fifth the distance had cost —
     because a band is carried round a corner by the join and what is
     drawn there is a quarter circle of the band's own reach. The
     reference renderer has the same reservation, added when the same
     mistake was made there; this is the second time that corner has
     been squared off by somebody who had just proved the runs exact.
     Taking the corner as an arc *instead* then cost the centred and
     inside-aligned cases their 0.025, since below a pixel of reach an
     arc and the corner it cuts are not distinguishable and the SDF is
     the worse of the two answers. So the arc is used where the arc is
     more than a pixel across and the box below that, which is the best
     of both measured rather than argued: 0.025 centred, inside, and at
     a width wider than the shape, 0.053 outside
     (`a_square_cornered_bands_edge_is_the_area_it_covers`, which also
     counts the band's own pixels, since a backend that drew no stroke
     at all would pass every comparison in it).
     What that left was the *curved* edge, and it turned out not to want
     a model of an arc at all — it wanted the thing a distance had been
     getting wrong all along. The coverage a distance was turned into was
     a ramp: half a pixel either side of the line, straight between. That
     is the area a straight edge lets through only for an edge standing
     square on the page. Turn the edge and the pixel meets it
     corner-first, and the area is quadratic over the part of the
     crossing where a corner is cut off and linear only over the part
     where two opposite sides are — two pieces, both closed form, with
     how far apart the square's corners lie on the line's own axis
     deciding which is which. At forty-five degrees the linear piece
     vanishes; square on, the quadratic one does and it is the ramp
     again. So it costs nothing anywhere and is exact where the ramp was
     not. Every curved case improved and none regressed: a disc's rim
     0.046 to 0.026, a rounded rectangle's corners 0.062 to 0.030, its
     band a tenth of full scale to a twentieth, a disc's band 0.115 to
     0.088 — and two more rough pages came clean.
     And then the worst pixel, which had sat at 0.358 through three
     rounds of real improvement without moving. Chased properly — one
     page, one layer, undressed a thing at a time — it is a stroked disc
     whose *blend* is Difference. Take the blend off and that same layer
     is 0.032; put it back and it is 0.330; its mask, which looked the
     likelier culprit, has nothing to do with it. A blend that is not
     Normal reads an unpremultiplied colour, which divides by the
     coverage, so a thirtieth of a pixel of disagreement at a
     barely-covered edge comes out a third of full scale. Both renderers
     do the same arithmetic and the amplification is not a defect in
     either. Which settles how to read that audit: the numbers that mean
     something are how many pages are rough and the mean inside each, and
     the worst pixel on the worst page is mostly a fact about blend
     modes. It stays in the ratchet as a ceiling, not as a target.
     The rest are a rectangle's *corners*, and chasing those found
     two more things in the reference renderer: a rounded rectangle is
     the same exact product away from its corners as a square one, which
     it was not taking; and the exact stroke band above had been written
     without that reservation, so it squared off a corner that is really
     a quarter circle of the band's own reach — a defect introduced and
     found within the day, by this instrument
     (`a_rects_stroke_is_exact_along_its_runs_and_round_at_its_corners`).
     What is left at a corner is two approximations of an arc, so the
     few pixels a corner costs are sampled sixteen to a side now rather
     than four — which turned up a standing test asserting that the pixel
     just outside a corner circle is drawn as *nothing*, where it is a
     hair inside the radius and really about a hundredth covered. The
     coarse box had been missing the sliver.
     Eighty-seven of the hundred and twenty pages the backend declined
     outright, which was its own thing to look at — and looking at it paid.
     Asking which single layer, taken away, makes a declined page drawable
     turned the eighty-seven into a tally, and the tally said *copy*: a
     copy was drawn only when it wore nothing at all. Faded, blended,
     masked, held to the layer under it or casting a shadow, the page went
     back. What a copy draws it draws by walking the layer it copies, and
     that walk ended the one that would have laid a surface down, so there
     was no surface of its own to fade or mask or grow a shadow from.
     There is now, on exactly the terms a group is isolated on and for
     exactly the same reason — what a copy draws may be a group whose
     children overlap, and a coverage taken as each child lands is taken
     twice where two of them meet. Seventy-three declined now instead of
     eighty-seven, and forty-seven pages compared instead of thirty-three.
     Which is also why what the backend will draw is a *table* now
     (`what_it_will_draw_is_written_down`): every kind of layer wearing
     each of the things a layer can wear, asserted, with the reason beside
     every no. A limit closed by accident should show up as plainly as one
     opened on purpose, and the table is what makes the copy row's six
     `no`s impossible to leave lying there unnoticed. Two more went the
     same way immediately after: an adjustment and a filter *wearing a
     blend* used to hand the page back, on the grounds that a blend puts a
     layer on a surface of its own and that would be a different picture.
     True — and the surface was the thing to stop rather than the page. The
     passes those two are drawn by carry their own parameters and never
     read the blend mode at all, which is also what the renderer being
     matched does: asked directly, a blend makes no difference whatever to
     what it draws there. So the blend is kept from forcing a surface and
     then ignored, and the pages drawn went from forty-seven to sixty-five
     of a hundred and twenty. Three `no`s are left and all three are
     decisions: an adjustment and a filter cannot carry an *effect* (what
     they draw *is* what is under them, so there is no silhouette to grow
     one from), and nor can a clone layer, which is never on a surface of
     its own.
     The tally, asked a third time, then pointed at a layer *held to* one
     that is dressed: the backend would clip only to a base that was
     plain, on the grounds that a faded or masked base's alpha depends on
     how it was composited. It does not depend on anything of the sort —
     what the layer above is held to comes from `layer_coverage_at`, the
     base drawn aside by the renderer being matched, so the base's own
     opacity, mask and blend are already in the number both sides read.
     Asked directly, a base faded, masked or blended lands within a
     thousandth of the reference, which is where a plain one lands. A base
     carrying an *effect* does not — the coverage then includes the shadow
     the base casts and what the CPU holds the layer above to does not, so
     that one still goes back.
     And the *kind* a layer may be held to, which was shapes, pictures and
     text: a base is a layer whose alpha is its own, so a group, a brush
     layer, a copy and a frame belong there too and were declining for no
     reason. A clone layer, an adjustment and a filter do not — what they
     draw *is* what is under them, and drawn aside they are nothing like
     what they are on the page; held to one of those the two renderers
     come apart by a sixth of full scale, which is what says the line is
     in the right place rather than merely somewhere
     (`anything_that_paints_can_be_held_to`). Eighty-four pages of a
     hundred and twenty are drawn now, thirty-six declined — where three
     passes ago it was thirty-three drawn and eighty-seven declined.
     A fifth pass took the last of them that was a gap rather than a
     decision: a copy *of* a layer that is itself held to the one under it
     was handed back, because this walk reaches the original through the
     path that reads `clipped` and would have held the copy back by a
     layer somewhere else entirely. The reference draws such a copy whole
     — a copy draws the layer, not the layer's place in a run of clipped
     ones, and a clip run is the parent group's business — which was asked
     of it rather than assumed: the copy's far corner, nowhere near the
     base the original is held to, carries the original's own colour. So
     `one` is told whether it is drawing a layer among its siblings or as
     what a copy draws, and the clip applies only in the first. Eighty-
     eight of a hundred and twenty drawn, thirty-two declined. What is
     left is decisions: an effect on an adjustment, a filter or a clone
     layer, and a layer held to one of those three.
     And the coarsest antialiasing in the renderer was found by the same
     reading and is gone: the scanline path fill is exact across a row
     and was *sampled* down it, four sub-rows deep, so a near-horizontal
     edge came out in quarters — a row nine tenths covered was drawn
     whole, on exactly the shapes a person draws with the pen. It is
     sixteen now and it renders *faster* than it did at four, because
     the crossings are taken from an edge table: only the segments that
     reach a row are asked about it, where before every segment was asked
     for every sub-row of every row, which on a spline of a couple of
     hundred anchors was nearly all of the work at any sample count. A
     canvas-filling spline at 1400x1000 went from 38ms to 29ms while the
     sampling got four times finer
     (`a_paths_edge_is_antialiased_down_the_page_too`). Worth knowing
     about the edge table: retiring an edge is a cost and not a
     correctness question — leave every edge live and every test still
     passes, slowly — so the benchmark was the witness for that half and
     nothing standing guards it.
     An *ellipse* now rasterizes the same way, which is the bigger of the
     two: its spans are two roots of a quadratic, so there was never any
     reason to sample it. Sampled, a circle was quantized by the pattern
     that took it — the flat top and bottom of it in quarters, where whole
     rows of the box flip together — and it cost a box of sixteen tests at
     every pixel of its box. Summing the alpha it lays down now recovers
     the true area to a fiftieth of a pixel where sampling was ten times
     further out (`an_ellipse_covers_the_area_it_really_has`), and eight
     big overlapping ellipses at 1400x1000 went from 180ms a frame to
     112ms. A turned one goes the same way ✅, and it
     turned out to be the same rule rather than a harder case. Inside the
     ellipse is where the device point, carried into the shape's space and
     divided by the radii, has length at most one — and that carry is
     affine, so the condition is a quadratic in the page's own axes, a row
     of it is a quadratic in x, and its two roots are the span. The map is
     read off three points (where the origin goes, and where a step across
     and a step down go) rather than assembled, so a turn, a shear and a
     mirror are nothing special and every ellipse is scanned. An affine
     map multiplies an area by its determinant, which is what makes a
     turned one's area known and so checkable
     (`a_turned_ellipse_covers_the_area_it_really_has`); put it back on
     the sampler and that fails.
     One thing tried and thrown away, which is worth recording: replacing
     the sampler's four-by-four box with sixteen samples in a *rook*
     pattern — one to a row and one to a column of a sixteenth grid, which
     is what a graphics API does for the same reason. It buys sixteenths
     on an edge standing square on the page, where the box can only say
     quarters. It also stops stratifying the pixel in two dimensions, and
     on a curve that costs more than it buys: the area an ellipse
     integrates to came out twice as far off, and the SVG round trip
     stopped holding. The box stayed. The right answer for a shape whose
     spans are computable is not a better sample pattern, it is not
     sampling — which is what the ellipse above does.
     Two: **ask an audit for the thing rather than an account of it** —
     the file audit compared a document written out as text and could not
     see wrong pixels; the clipboard audit compared a layer field by field
     and could not see a picture that arrived blank; the SVG witness read
     a dash pattern out of the markup rather than off the page, where a
     line on where it should be off would have passed. All three now
     compare the picture. The newest of those: a *clipped* layer travels
     to SVG as a mask made out of the layer below it, and what said so
     was a test reading the markup back for a `<mask>` element over the
     right box. The resvg page carries one now — take the mask off and
     the comparison fails, shift its box six pixels and it still fails —
     with three spot checks saying the overlap, the base alone and the
     overhang each show what they should.
     And the instrument itself changed, which was overdue. That witness
     compared a page-wide mean against a threshold, and a mean is a poor
     thing to measure export fidelity with: every diagonal edge costs it
     a little, so it rises as the page gains elements and the threshold
     has to be loosened to let innocent additions through — an audit that
     catches less the more it is given to look at. It was at 3.29 against
     a ceiling of 3.5. What is *not* allowed to differ is the inside of a
     shape, where neither rasterizer has an edge to disagree about and
     neither has a half-opaque layer to composite in a space of its own.
     So every layer is drawn alone now, its opaque interior found, and
     only the points where the page shows that layer's own colour unmixed
     are compared — 2631 of them across seven layers, to four levels out
     of 255. That leaves out exactly the two things allowed to differ,
     and grows rather than thins as the page does. One exception, about
     the reader rather than the export: a raster enlarged is resampled,
     and the two do not use the same kernel, so every point a raster
     *covers* is left out — which is more than it paints, since a
     transparent texel enlarged lets a little of an opaque neighbour
     bleed into what is under it. Put five levels of error on one shape's
     fill and the mean sails through at 3.31 while the interiors name 121
     points; that is the whole reason for the change.
     A PDF is read back too, and that line here said
     otherwise for longer than it was true: Ghostscript rasterizes both
     the page and the frames when the machine has it, self-skipping when
     it does not, and what it draws is held against what the engine
     draws (`ghostscript_draws_the_same_page_the_engine_does`). That one
     takes the interior reading now as well, on the same argument and
     with sharper point: its mean was at 2.77 against a ceiling of 3.0.
     Loosen that ceiling to 4.0, where a busier page would push it, put
     six levels of error on one ink, and the mean lets it through while
     the interiors name 1263 points and the layer they are on. A PDF's text is asked where it
     *landed* now too (`the_pdf_sets_text_where_the_engine_sets_it`),
     which neither of the other two readings can answer: a glyph's stem
     is a pixel or two across, so it is all edge and nothing inside, and
     a page-wide mean would not notice a line of type moved two pixels
     along. What survives two rasterizers hinting and darkening stems
     their own way is where the ink *is*. So the page is drawn twice on
     each side, once with the line and once with it hidden; what the text
     put down is the difference; and the two are held to the same centre
     of mass, the same extent and about the same quantity of ink — over
     the line and over each quarter of it, found from where the ink is
     rather than from the page, so that a glyph moved inside the line is
     not averaged away by the rest. A pixel of slack, which is what a
     rasterizer that darkens stems costs; four sabotages land outside it
     — every glyph a point and a half right, a point and a bit down, an
     advance drifting a quarter point a glyph, and the face set three per
     cent wide.
     A fifth reading was written and thrown away, which is worth
     recording: the same line at two placements a whole number of pixels
     apart, holding the *difference* between the renderers to be the same
     at both. A whole number of pixels is what keeps the rasterizer's own
     bias constant — and it is also what makes every placement mistake
     identical at both, so the check could not fail. The two things
     wanted of it were the same thing pointing opposite ways.
     One of those has gone in. **A picture inside an SVG used to be
     dropped on the floor**: `usvg::Node::Image` was matched and ignored,
     so a file with a photograph in it imported as the shapes around the
     photograph and nothing where it was, silently — the import still
     succeeded and the page still drew, and only somebody who knew what
     the file held would know. It comes in now, in its place.
     It wants both halves of what a raster is, and that is why it was
     left: a raster layer is a *reference* to pooled pixels, and the
     importer has no document to pool them in. So the file's pictures
     come back beside its shapes, each saying how many shapes go below
     it, and the engine adds the resources and puts the one order back
     together. Being in the same batch, the whole import is still one
     undo step, picture and all. A nested `<svg>` is not a picture at
     all and comes in as more shapes; GIF and WebP arrive as bytes this
     build has no decoder for and are still passed over, which is the
     same answer as before for those two.
     What is worth recording is how nearly the test was useless, twice.
     The picture is a two-by-two of known colours rather than a
     photograph, so a flip or a swapped channel order says so plainly —
     and a two-by-two in a *square* box makes the two scale factors
     equal, so the first version passed with the axes swapped. Making
     the box oblong was not enough either: SVG's default preserves the
     aspect and letterboxes, which usvg resolves before handing it over,
     so the scales were equal again. It takes `preserveAspectRatio="none"`
     to get a genuine stretch, and only then does the placement have two
     numbers a test can tell apart.
     And that was what showed the code was carrying a no-op. It had been
     scaling the picture's own grid into the size usvg reports — but
     usvg reports the image's *intrinsic* size and puts the whole
     placement, stretch included, into the absolute transform. The ratio
     was always one. Four sabotages say the placement is now watched for
     real: an identity transform, a transposed scale, a dropped
     translate and pixel rows reversed are each caught.
     And a second thing the importer was writing over: **a broken line
     came in solid**. `dash: Vec::new()` was put where the file's
     `stroke-dasharray` belonged, so every dashed rule, cut line and
     border in an imported drawing arrived unbroken. That one is easier
     to sit on than the missing picture, because nothing is absent from
     the layer list and nothing fails — it reads as a slightly wrong
     line rather than as a loss. The two mean the same thing (lengths
     along the outline, on and off in turn and repeating) and are in the
     same units as the width, so they take the same scale; an odd-length
     pattern needs no special case, since SVG repeats it to make the runs
     alternate and a pattern walked round and round does that by itself.
     What is still not carried is `stroke-dashoffset`, which shifts where
     the pattern begins and has no field here to land in — a line whose
     dashes start a little further along is much nearer the file than a
     line with no dashes, so it comes in unshifted rather than refused.
     The same lesson as the picture, on the same afternoon: the first
     version of the test was written on a path with no transform, where
     the scale is one and scaling is a no-op, so it passed with the
     scaling taken out. A second case puts the same line inside a
     doubling. Three sabotages are caught now — the pattern dropped, the
     pattern unscaled, and the pattern halved — where before only the
     first two of those three were.
     And a third, which was a shape coming in *wrong* rather than a
     shade being off: **a path SVG fills by winding came in filled
     even-odd**. Subpaths here are even-odd — a point inside two of them
     is outside the shape — and SVG's default is nonzero, where inside
     two is still inside. The two part company exactly where subpaths
     overlap, so two rectangles in one path, wound the same way and
     overlapping, drew solid in the file and drew a hole here. `fill-rule`
     was never read at all.
     Where every ring is wound the same way, nonzero is *exactly* the
     union of them — a point inside `k` of them has winding `±k`, which is
     non-zero for every `k ≥ 1` — so that case is converted rather than
     approximated, through the same shape booleans a selection is built
     with. Rings wound both ways are left alone on purpose: that is the
     ordinary outline-with-a-counter, where the two rules already agree,
     and the cases where they do not — a hole inside two overlapping
     outlines, still filled — cannot be said as a union at all. Guessing
     there would break every letter with a hole in it to fix something
     nobody draws. And nothing is converted unless two same-wound rings
     actually overlap, since the union goes through flattened outlines and
     a path needing no correction should not come back a polygon.
     Four sabotages, one for each branch of that rule, and getting all
     four watched took three rounds. The conversion skipped, and never
     converting, were caught by the overlapping case. The winding sign
     ignored was not, until the outline-with-a-counter case went in
     beside it — without that, unioning everything looked free. And the
     overlap test was not watched by either, because unioning two rings
     that do not overlap draws the same picture; what it costs is the
     curves, so what watches it is two circles nowhere near each other
     keeping their handles. Each of the three cases is held against resvg
     rather than against a number picked by hand, so what is asserted is
     the file's meaning rather than this importer's idea of it.
     And a fourth, the loudest: **what a clip path hid did not stay
     hidden**. `clip-path` was never read, so a clipped group came in
     with everything showing — the others made a line or a corner
     slightly wrong, this one puts artwork on the page the file says is
     not there. Outside the band, resvg drew nothing and this drew solid.
     A clip is the one thing about a group that survives the group being
     flattened away, which is what makes it carryable at all: it
     multiplies coverage by nought or one, and that distributes over the
     children exactly — each child seen only inside the region is the
     same picture as the group seen only inside it. Opacity and blending
     do not distribute that way, which is why they are still folded into
     the colours instead. So the region rides down the walk and lands on
     each shape as an ordinary vector mask, and on an embedded picture
     too. A clip inside a clip is the intersection; a clip of several
     outlines is their union, not the even-odd of one compound shape;
     a clip on the clip path itself narrows it again.
     Four sabotages, and the fourth needed a case of its own — the same
     shape of gap as the winding rule the commit before. The mask not
     attached, the clip not read, and the outer clip dropped when nested
     were all caught. Union turned into intersect was not, because the
     two outlines in that case were *disjoint*: intersecting them fails,
     the fallback keeps both rings, and even-odd over two disjoint rings
     is their union anyway. It takes two outlines that **cross** to tell
     the two apart, where a union shows the overlap and even-odd would
     punch it out. Every case is held against resvg rather than a number
     picked by hand.
     A fifth, found by the same oracle and half fixed on purpose: **a
     `mask` was ignored**, so content the file only half shows, or does
     not show at all, arrived whole. An SVG mask is greyscale — coverage
     is the luminance of whatever is drawn in it — and nothing here can
     hold that without pooling pixels for a raster mask, which is the
     picture problem again. But a mask drawn as opaque white shapes is a
     *region* and nothing more, which is how most masks in most files are
     used, and that is carried now through the clip machinery: a clip and
     a mask are both "show only here", so they meet as one region.
     The guard is the part worth having. Every condition fails towards
     doing nothing, which is exactly what happened before: a fill that is
     not white (on a luminance mask), not opaque, or a gradient; a faded
     group; a stroke, an effect, text or a picture — each of those is
     grey somewhere, and a region would be wrong about it in the
     direction of showing too much. A mask painted grey is asserted *not*
     to become a region, and the content it should fade is asserted to
     still arrive whole, which keeps the known loss visible instead of
     quietly turning it into a confident wrong answer.
     And the same tie-breaking lesson for the fourth time in a row: three
     sabotages were caught by the first two cases and the fourth was not.
     A mask has a region of its own, outside which nothing shows however
     white it is painted — and the test's mask content sat entirely
     inside its own default region, so ignoring the region changed
     nothing. It takes content that *overflows* the region to tell them
     apart.
     Two more of the same class are found and not fixed, which is worth
     writing down rather than leaving for the next search to rediscover.
     A `filter` is dropped (a shadow's is not, now), so a blurred element comes in sharp — at two
     pixels outside a blurred rect resvg draws a quarter coverage and
     this draws nothing. That one is not an importer gap but a model one:
     a filter layer here applies to everything below it, and clipping it
     to the shape would cut exactly the spread that makes it a blur, so
     there is no way to say "blur this one layer" yet. And a pattern fill
     came in unpainted, since `Paint::Pattern` had no answer here; it
     comes in now as a picture of the tile (see "And a pattern, painted." in
     §0's import notes).
     Then whatever the next user of the editor misses first — a brush
     that paints pixels rather than laying down live strokes. This line
     used to ask for text shaping worth the name as well, and that has
     been true for a while: rustybuzz shapes it, a face family answers
     for bold and italic with its own cuts and what it has no cut for is
     synthesized, and a style run names a face of its own — the run's
     styling is what is shaped as well as what is drawn, so a ligature
     never crosses a boundary between two faces and the pen carries the
     run's own thickening. Worth striking rather than leaving to read as
     missing.
     Nine, and new: **where a boundary is mirrored by hand, ask serde for
     the vocabulary and hold the mirror to it.** Every method above works
     inside the Rust. The editor's API is not inside the Rust: the UI is
     TypeScript across a wasm boundary and every mutation it makes is a
     serde-JSON `Command`, so the JSON *spelling* of these types is the
     API, and `app/src/engine.ts` is a hand-written copy of it kept by
     somebody remembering to keep it.
     `every_command_survives_the_boundary_the_ui_talks_over` already
     carries each command over that wire — but it asks Rust both times.
     Serialize here, read back here, and a field renamed on *both* sides
     at once passes perfectly while the far side goes on saying the old
     word. That is the half nothing could see.
     Most of the boundary turns out to be safe without anybody's help,
     and knowing why is the useful part: `wasm-bindgen` generates the
     `.d.ts` for the typed methods, so the compiler holds the UI to them
     — rename `on_mask` on `paint_begin` and `npm run build` stops. The
     JSON commands have no such guard. Serde does refuse an unknown
     variant and a missing required field, so most drift is at least an
     error in the browser; what is *silent* is a field carrying
     `#[serde(default)]`, where the UI keeps sending the old name, serde
     ignores what it cannot place, and the value is whatever `Default`
     says. That class is small and nameable: `Mask::feather` and the six
     nullable fields of `StyleRun`, which are exactly the seven the
     mirror marks optional.
     So the vocabulary is asked of serde rather than of the source — the
     commands and the fixture written out, and the names read off the
     JSON, which is the same text the UI has to write — and then every
     name the mirror declares has to be one of them
     (`the_uis_mirror_of_the_wire_says_what_this_crate_says`). One
     direction only: Rust holds plenty the UI has no business saying.
     Where the mirror is deliberately narrower it says so in its own
     comment, which is the difference between a gap and a decision —
     `MaskKind` omits `Painted` because the engine is the only side that
     builds a brushed region, and `regionMoved` returns null rather than
     learn a shape it never makes.
     **No defect**, which was the expected answer and is worth writing
     down: every name agrees today, and the seven optional fields line up
     exactly with the seven `#[serde(default)]`s. The reason to keep it is
     drift, and what it is worth was measured rather than assumed. Rename
     `Mask::feather` on the wire alone — a `serde(rename)`, which is what
     drift looks like with the code untouched — and it is the **only**
     failure in the workspace: 419 other tests pass while a soft-edged
     region picked in the browser would come out hard. Three more land
     the same way, one per half of the check: a command's own field
     renamed, a blend mode spelt differently, and the misspelling put in
     the *TypeScript* instead, which is the direction where the UI is at
     fault. The browser suite would catch the first of those too — in two
     hours, when somebody runs it. This says it in a hundredth of a
     second, which is the whole argument for it.
     Two things about the writing of it, both mine rather than the code's.
     The first draft's non-vacuity floor was *guessed* — forty spellings
     where the true count is thirty-six — and a floor guessed rather than
     counted fails honest code, which is the same carelessness as a bound
     too loose to fire, pointing the other way. And the first version
     counted twenty-six command variants against a list of twenty-seven
     and I took it for a gap: `RestoreSubtree` arrives *inside a batch*,
     deliberately and with the reason written down, since it only means
     anything on a document its subtree has just been taken out of. The
     audit beside it recurses into batches and this one now does too.
     Worth keeping from that wrong turn: the older test's own coverage
     assertion is `checked >= EVERY_VARIANT.len()`, and `checked` counts
     *commands*, not variants — a count, not a cover, satisfied by
     duplicates. It is fine, because `the_list_holds_every_command_there_is`
     does the covering properly. But that is the shape to distrust on
     sight, and this is the second time in this document that reading a
     `>=` as a cover has been the mistake.
  4. **The chrome, against Canva, Affinity and Photoshop.** The menus
     and the rail were reviewed against all three in September 2026 and
     brought into line — eight menus, submenus, Adjust and Filter, a
     rail in sections with a hand and a zoom at its end, and the tools
     on it a preference (all above, under *Chrome*). What the review
     found still missing, in the order it would pay, so that the next
     chunk here is a choice rather than a survey:
     - ✅ A **Help** menu — every one of the three has one. Keys and
       gestures moved onto it from View, and About says which version
       this is, app and engine both.
     - ✅ **Gradient tool** (G): drags a new gradient across a shape,
       linear or (alt) radial, from the ink in hand — see *Chrome*.
     - ✅ **Fill (bucket) tool**: a press gives the layer under it the
       ink in hand, alt its stroke, and bare paper with a region picked
       out fills the region. G again, after the gradient — see *Chrome*.
     - ✅ **Eraser** and **Heal** as tools of their own (shift+N,
       shift+S), the two switches gone — see *Chrome*.
     - ✅ **Node tool** (A): a press brings a shape's anchors out, a
       press on the outline adds one — see *Chrome*.
     - ✅ **Scale the page and everything on it** (Page › Scale the
       page…), one factor both ways — see *Chrome* for why not two.
     - ✅ **Open recent**, on the desktop shell where a path can be
       reopened; a browser cannot reopen a file by name, so there it
       stays a draft. It wanted the shell to open and save by path at
       all, which it does now, with the system's panels — see §0
       *Desktop shell, files by path*.
     - ✅ **Named styles**, beside the palette and on Layer › Styles,
       kept in the document — see *Chrome*.
     - ✅ **Brush presets**, chips over the paint row, kept in
       preferences — see *Chrome*. Pressure response is not a number
       here (it is the stroke's own, from the pen), so a brush is width
       and softness.
     - ✅ A **shape library** (the Shape tool, U): triangle, diamond,
       arrow, chevron, callout, heart, each a path preset — see
       *Chrome*. That was the last item on this list: everything the
       September review found worth doing is done (Open recent came
       last, once the shell could open and save by path).
     - A **Text** menu (Affinity) or Type menu (Photoshop) is *not*
       wanted yet: everything about text is on the panel where the
       text is, and a menu of the same controls would be the
       duplication the View menu just lost.
     - Already there, so not on the list, though a survey would have
       guessed otherwise: text on a path, smart guides, rulers and
       guides, a grid, a history panel, artboards with export scale,
       components with overrides, booleans, live effects, blend modes,
       masks of three kinds, a palette with named colours, soft
       proofing, a preferences window, an export window that shows the
       file's size. What is deliberately not planned: warp, liquify and
       perspective distortion of pixels, and content-aware fill — the
       first three are destructive by nature and the last is a model
       this editor does not ship.
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
  working. Document actions live in a nine-menu bar — File, Edit,
  Select, Layer, Adjust, Filter, Page, View, Help — reviewed against
  Photoshop's, Affinity's and Canva's and laid out the way theirs are,
  each menu named after the one thing it holds. File is the file: new,
  open, save, what comes in (an image, a font), what goes out. Edit is
  the verbs every application has. Select is picking layers and picking
  a region out of the page, with a rule between the two. Layer keeps its
  four families — Arrange, Align, Combine shapes, Mask — behind a row
  each, since eight alignments in a flat list are a wall, and carries
  lock, hide and rename as well as group, clip and the live copy.
  **Adjust** and **Filter** are the two menus a photo editor is
  incomplete without: every adjustment and every filter as a row, and a
  second list behind "Only on *layer*" scoped to the picked layer the
  way the panel's "+FX" offers it — before this they were only on that
  panel list, which is where nobody looks first. Page is the page's own
  properties: its size, its turns and mirrors, and the press profile it
  will be printed through, which used to sit in File under the exports.
  View is zoom, units, guides, the grid, the before/after eye and the
  two rows that open Preferences; the monitor profile rows that were
  there are in Preferences under Colour and nowhere else, since a
  setting in two places is a setting that can disagree with itself.
  Help is the keys sheet, which had been standing in for it from the
  wrong end of View, and About — the app's version from package.json
  (put into the bundle by vite.config.ts) beside the engine's own
  (`engine_version()` over the WASM boundary), one number when the
  bundle is whole and two when it is not, which is what the About
  window is for.
  Nine menus and every group of buttons at once are wider than a
  laptop's window, and the first version of this found out the hard
  way: a bar a few pixels wider than the window scrolled the whole page
  sideways, which slid the canvas under the tool rail, where a drag
  from the page's corner drew nothing — one block of the suite, five
  thousand lines from the change, was what noticed. The bar does not
  wrap on a wide window (a bar that wraps never shrinks anything first)
  and does not scroll: the document name gives up room first, then the
  wordmark goes below 1520px, then the document's facts below 1380px
  (the profile badges beside them stay, being said nowhere else). The
  suite asks at 1440 and 1366 that the window holds the whole page and
  the bar is one row, so the next thing added to the bar is told rather
  than found out.
  A menu row can open a menu of its own (`MenuEntry` kind `sub`), on
  hover or on a press, and the popup is fixed to the window rather than
  to the row because the menu it sits in scrolls when tall and a popup
  inside a scrolling box is cut at the box's edge; the desktop shell
  builds the same description into native submenus. The tool
  rail, layer actions and top-bar toggles are icons from `app/src/icons.tsx`
  (one stroke weight, one 24-unit grid, drawn in currentColor). Accent is
  reserved for state — active tool, open menu, live toggle, selected layer.
  The rail (`app/src/tools.ts`, which the rail, the keys and the
  preferences window all read) is five sections with a line between —
  what picks and moves; what makes things; what paints on them; what
  changes the picture; what only looks at it — ending, as every editor's
  does, with a **Hand** (H, a drag carries the view) and a **Zoom** (Z,
  a press looks a quarter closer about the point pressed, alt steps
  back). The painting section ends with a **Gradient** tool (G): a drag
  across a shape — the one under the press, else the one picked — gives
  it a linear gradient from the ink in hand to white, aimed the way it
  was dragged; alt makes it radial about the press, shift holds the
  angle to an eighth of a turn, a shape already wearing a gradient
  keeps its stops and is only re-aimed, and a press that never moved
  changes nothing. It previews live and commits as one entry, through
  the same `SetKind` the panel's fill-type row and the on-canvas handles
  use, so nothing new is in the document for it. Beside it a **Fill**
  tool, the bucket: a press gives the layer under it the ink in hand —
  a shape's fill in place of any gradient, a block of text's, a frame's
  ground — or with alt a shape's stroke, adding one where there is
  none; a press on nothing that takes a colour, with a region picked
  out, fills the region as a layer over it, which is what a bucket is
  for on a photograph. It shares G with the gradient tool: shift with
  the key walks its family from whichever is in hand (`KEY_FAMILIES` in
  tools.ts), shift+M the region tools the same way — Photoshop's
  convention, chosen over "press it again" because sixteen blocks of
  the suite press M to mean the marquee whatever is in hand, and a
  plain key that means one thing always is the better promise. The
  keys sheet had said M "again" walked the family while nothing did;
  it says shift now, and it is true. The **Eraser** and the **Heal**
  are tools of their own on the rail, where a person looks for them,
  rather than switches on the paint and the clone: shift+N and shift+S
  from the tool each shares a key with. The tool keys are the person's
  to change: the Tools page of the preferences shows each tool's key
  as a button, press it and the next key pressed is the tool's
  (`prefs.toolKeys`, only what differs from the defaults; `boundKeys`
  in tools.ts lays the rebindings over `TOOL_KEYS` and is what the
  keyboard handler, the rail's tooltips and the keys sheet read, so
  the sheet lists a rebound tool under the key it now answers to). A
  key another tool holds changes hands — that tool takes the key this
  one had, so the swap leaves nobody unreachable — and Backspace puts a
  key back to what it shipped as, taking it back from any rebinding
  that held it. A family's key is held by its first member and the
  rest are a shift away, so rebinding the marquee moves the lasso and
  the wand with it. What is *not* rebindable is the command keys —
  Ctrl+S, Ctrl+G, the brackets — which are still literals in the
  handlers. The **commands** answer to a chord the person chooses too
  (`app/src/commands.ts`, the preferences' Keys page): one table of
  twenty-one — save, group, copy a look, send to the back, undo, pick
  out the rest — where there had been a dozen nested tests on modifiers
  and letters in two keydown handlers and the same chord spelled again
  as a hint on every menu row. A chord is one string (`mod+shift+e`,
  the modifiers in a fixed order, `mod` being whichever of control and
  command the machine has), so the handler is a lookup rather than a
  ladder — and the ladder's own hazard goes with it: clipping had to be
  tested before grouping because they differ by one modifier, and two
  chords simply cannot be confused. Brackets are named by what is
  printed on them rather than by what shift makes of them, since those
  differ by keyboard. Rebinding follows the tool keys' rule — a chord
  another command holds changes hands, Backspace puts one back to what
  it shipped with and hands that chord's old holder back its own — and
  the menus, the context menu and the keys sheet all write the chord as
  it now stands. The chrome has a **light theme** as well as the dark one,
  and which is shown is the person's (`prefs.theme`: as the system,
  dark, or light; the General page). Every colour of the chrome is a
  token on `:root` now — the six that were, and a dozen more that had
  been literals down the sheet: the hairline, the rulers, the area
  behind the page, the shadows, the scrim, the checkerboards, a note's
  amber, the label on the accent — and the light set is one block
  under `:root[data-theme="light"]`. The app resolves "system" to one
  of the two before putting it on the root, following the machine
  live, so the sheet never says the light set twice. What is drawn
  *over the picture* — handles, anchors, ants, the crop's frame, the
  brush ring — keeps its own white and black on purpose: it sits on
  the page, and a page can be any colour under either theme. The
  suite pins the browser to a dark machine, since Playwright's own
  default prefers light and every chrome pixel it reads was authored
  against the dark set; the light set is tried in a block of its own
  (`asked for light, the chrome is light`), which also moves the
  machine's preference both ways and watches the chrome follow. The engine is told what it
  always was — `erasing` and `healing` are read off the tool in hand —
  and the two switches are gone, since a mode that is a tool does not
  also want a switch. Clone no longer heals by default: it is the plain
  stamp, and Heal is the one that lands in the colour of where it
  lands. A **Node** tool (A) is for a shape's anchors, which Move
  showed but nothing named: a press on a rectangle or an ellipse makes
  it a path so its anchors come out (`Session::as_path`, four anchors
  on an ellipse at the ends of its axes, four or eight on a rectangle,
  the arcs cubic beziers at 0.5523 of the radius, which is under three
  parts in ten thousand from a true arc — the picture does not move,
  only what can be taken hold of, and
  `a_shape_made_a_path_draws_the_same_picture` holds it to that, having
  first been caught comparing two blank pages: a vector node starts
  with no fill, and a test that passed any arc at all was the first
  version); a press on the picked path's own outline puts an anchor
  there, where Move wants a double-click; the anchors answer to a drag
  and to alt whatever the tool. The browser block reads the ellipse's
  rim a quarter of the way round from the axes, where a wrong arc
  strays furthest, and allows a fifth of a pixel's coverage: both
  pictures are sixty-four chords, laid in different places.
  **Scale the page…** on the Page menu is the same design at another
  size — Photoshop's Image Size, Canva's Resize — where Canvas size…
  is the same artwork on a page of another size. It is a command of
  the document's own, `ScaleCanvas { factor, width, height }`: one
  factor, the same both ways, and the size it lands on said rather
  than worked out, as a straighten's is, so the inverse (the
  reciprocal, with the old size) puts the page exactly where it was.
  One factor rather than one per axis for two reasons the audits
  taught in that order: a length carried through a page transform — a
  brush's radius, a shadow's blur, an edge's softness — goes by the
  larger axis scale, so a scale by 1.875 one way and 1.5 the other
  came back with a radius of 11.25 that had been 9 (the first version
  was two sizes, and `every_command_undoes_to_exactly_where_it_started`
  said no); and a design stretched one way is not the same design. It
  is in `fixture::exact` as the second command allowed to be near
  rather than exact under undo, beside the straighten. The window asks
  for a width, a height or a percentage and keeps them one number;
  what a layer holds in its own units — a blur's radius, a stroke's
  width — does not scale, and the window says so.
  **Styles** — looks kept by name — sit beside the palette and mirror
  it through the whole stack: `Look` (fill, stroke, gradient, effects,
  opacity, blend — what copying a style always copied, now a type of
  the document's rather than the engine's own) and `KeptStyle { name,
  look }`, a `styles` list on the document with a whole-list
  `SetStyles`, in the shared command list and the fixture, eight
  fields in the file inventory, `Session::keep_style`, `apply_style`
  and `forget_style` over the same `apply_look` the paste goes
  through. A style keeps its colours flat: it names a look, not the
  palette entries the look was made from, so a palette change does
  not reach it and the document need not walk its styles to settle
  them; a layer given a style keeps the look when the style is
  forgotten, since a style is copied onto a layer, not followed. Kept
  under the next free name — "Style 3" — the way a colour goes into
  the palette, since a name typed into a prompt is a name nobody
  types; the chips show the fill, the stroke and the name; press to
  give, shift-press to re-keep, alt-press to forget; and Layer › Styles
  has the same by name. The browser block saves and reopens the file
  and finds the style still there.
  **Brushes** kept by name — a width and a softness — are chips over
  the paint tools' row (`prefs.brushes`, three to start with: Fine,
  Medium, Soft): a press takes one up and the chip shows it is the one
  in hand, + keeps the numbers in hand under the next free name, alt
  forgets. About the person and not the file, so they live in
  preferences beside the hidden tools, read back name by name and
  clamped to what a brush takes; the browser block reloads the page
  and finds a kept brush still there. A chip is drawn as the brush it
  stands for, a dot the brush's width blurred by its softness, since
  that is what a hand chooses by. The chips found a defect older than
  they are: the rail's width was its widest child's, so a row of chips
  wider than the tools widened the rail, which narrowed the canvas,
  which re-fitted the page — and every coordinate the paint block had
  taken with Move in hand was off by a page's shift once Paint was
  picked. The softness slider had been doing the same by ten pixels
  since it was written, tolerated only because ten was not enough to
  move a probe off a stroke. Everything under the tools now wraps to
  the tools' own width, and the brushes block holds the rail to the
  same width with Move in hand and with Paint.
  A **Shape** tool (U) shares the shapes' slot and draws whichever of
  the library's shapes is chosen on its own row of chips
  (`app/src/shapes.ts`: triangle, diamond, arrow, chevron, callout,
  heart — the ones a survey of the three editors found in all of
  them): each a path in a unit box scaled to the box dragged out, the
  way the polygon and the star already land as paths, so every anchor
  is draggable the moment it is drawn and every exporter already knows
  what it is; the heart carries bezier handles, and the browser block
  reads a point inside a lobe's curve that a straight chord would leave
  bare. Each is a path preset, not a new kind of layer.
  Which tools are on the rail is a preference (Preferences ›
  Tools, or View › Tools on the rail…): a tool unticked leaves the rail
  and waits behind a "…" slot at its end with the others put away, its
  key still picking it and the slot then showing it in hand — put away
  is not gone — and the one tool that cannot be put away is Move. The
  same page decides which of the bar's three button groups (document,
  region, zoom) show, everything on them being on a menu as well.
  Tools have single-letter shortcuts (V/M, R, E, U, P, A, B, N, S, G, T,
  C, I, H, Z; shift+N, S, G, M for the tool each shares a key with),
  suppressed while typing. Layer rows carry a picture of the layer (a kind glyph where
  there is nothing to picture) and a mask marker.
- **Tooling:** `tools/chitrakar-plugin/` is a Claude Code plugin bundling
  the verification gate, status, ship, the engine conventions skill, and a
  SessionStart hook (install: `/plugin marketplace add
  SandeepSubba/Chitrakar`, `/plugin install chitrakar@chitrakar`).
- **A paste event is not always somebody asking to paste.** On X11 the
  middle button pastes the primary selection, and the middle button is
  this app's own way of carrying the view — so every drag of the view
  arrived as a paste event with nothing in it at all, and an in-app
  clipboard holding a layer meant a stray copy of that layer every time
  the view was dragged. Preventing the default on the gesture does not
  stop the event arriving (pointerdown, mousedown, mouseup and auxclick
  were each tried). Not acting on it does: the paste *event* serves the
  one thing only it can see — a picture another application put on the
  clipboard — and the in-app clipboard is the keystroke's own business,
  which it already was, since Ctrl+V pastes it a beat later unless the
  event has served the paste. Asked directly by the suite now (8y2)
  rather than found by an unrelated assertion tripping over the extra
  layer.

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
- **Two windows, and one place the app's own settings live** ✅.
  Getting a picture out was thirteen rows on the File menu — PNG, PNG at
  2×, at 3×, the same three again for what is picked, this artboard,
  every artboard, JPEG, SVG, PDF, the frames as pages, TIFF — which is
  every combination somebody might want, guessed in advance and frozen
  into a row, and still no way to ask for a JPEG at 80 or a PNG at half
  size. `app/src/ExportDialog.tsx` asks the three questions that
  actually decide an export instead: what form it takes, how much of the
  page goes, and how big. What a format cannot do is disabled rather
  than hidden — an SVG has no pixels to be a multiple of, and saying so
  is worth more than the control's absence — and a CMYK TIFF names the
  press profile it is waiting for rather than failing when the button is
  pressed.
  The part worth keeping is the number at the foot of it. Affinity and
  Photoshop both estimate the file's size; this encodes it, because the
  encoder is in-process and the bytes are already being made — so the
  figure shown *is* the file, which is what makes a quality slider worth
  dragging. The same encode is then what gets written, so the window
  cannot promise one thing and save another
  (`and weighs what the window said`). Above four megapixels it waits to
  be asked rather than re-encoding a print-sized page on every drag of a
  slider, which would freeze the very control being dragged.
  Beside the settings is the picture, and it is the same bytes again:
  the encode that gave the size is handed to the browser to decode, so
  a JPEG at quality 5 shows its blocks before it is taken — the one
  thing the number cannot say — and a PNG shows its transparency over a
  checkerboard where the JPEG shows white. The suite reads the picture
  back rather than trusting that it is there: down a rectangle's edge,
  the colour strays from a clean step at quality 5 and not at 100
  (`the picture shows what the quality does to an edge`), which a
  preview drawn from the page instead of the file could not do. A PDF
  and a TIFF the browser cannot show; for those the page stands in,
  drawn as the engine draws it, and the caption says so.
  The set an asset pipeline wants is one press too: `Set` beside the
  scales writes `@1x`, `@2x` and `@3x` together, the size shown is all
  three files, and the ceiling counts all fourteen page-areas of
  encoding, so on a 640 by 480 page the window already waits to be
  asked. Kept in the preference as a scale of zero, which is not a
  scale and never reaches the engine as one.
  **Slices** are the regions the document already keeps by name: ask
  the area for "Each kept region" and one press writes a file per
  region, each at its own box's size and named for it
  (`untitled-badge.png`). Nothing new is written down for a slice —
  a region kept by name *is* one, which is why this needed no document
  change at all, only an engine call that reads a kept region where it
  is kept (`Session::kept_region_png`) rather than picking it first:
  picking is a command, a command is an edit, and exporting should
  move neither what is picked out nor the history. It multiplies with
  the `@1x/@2x/@3x` set rather than choosing against it, which is what
  three sizes of four slices means.
  Exports kept by name are chips above the formats (`prefs.exportSetups`,
  the same shape as the brushes): + keeps the window's four answers
  under a name made from them — `JPEG 2× q80`, `PNG set`, a plain
  `PNG` for one file at one to one — a press gives all four back, and
  alt-press forgets one. The format preference is validated on the way
  in now too; it had been taken on its type alone.
  Two exports stayed rows on the menu, and the reason is worth writing
  down: a frame carries the multiple it wants to come out at as a
  property of the *document* (`export_scale`, saved with the file), and
  its name comes from the frame. The window offers a multiple of its
  own, and the two multiplied together is not a thing anybody means — so
  "Export this artboard" and "Export every artboard" (one file each, and
  the frames as PDF pages) are still rows. A dialog replacing a menu is
  right up to the point where the menu row was answering a different
  question.
  `app/src/prefs.ts` is the other half. Units, the grid, guides and the
  monitor profile were three separate stretches of the View menu; how
  far an arrow key moves a layer and how near a thing has to come before
  it catches were constants in the source (`SNAP_PX`, a literal `10`)
  reachable from nowhere at all — a setting you cannot find is a setting
  you do not have. They were also four `localStorage` keys under two
  spellings (`chitrakar:grid`, `chitrakar.units`), which is how a fifth
  convention gets invented. One object under one key now, read back
  field by field against the defaults' own types — this is JSON another
  version of the app wrote, and a string where a number belongs would
  otherwise reach the engine — and clamped on the way in, so a grid of
  −4 or a quality of 900 stops at the boundary rather than downstream.
  The old keys are read once so a grid and a unit chosen before any of
  this existed survive it; a preferences window that silently resets
  what you had is worse than none.
  `PreferencesDialog.tsx` is a rail of seven groups, which is Photoshop's
  shape and Affinity's both, and earns the furniture at seven — a flat
  list of eighteen controls is a search problem. The seventh is Tools:
  a checkbox per tool, in the rail's own sections, and one per group of
  buttons on the bar; `hiddenTools` is a list of what is *hidden* rather
  than what is shown, so a tool a later version adds appears for someone
  whose settings predate it, and it is checked name by name on the way
  in since `typeof []` and `typeof null` are both "object". What is deliberately
  not in it: anything the document owns. Its size, its press profile and
  its guides travel with the file; a preference is about the person and
  stays on this machine. The one place they touch is "New documents",
  which is not the document's size but the size the next one starts at.
  `Ctrl+Shift+E` opens the export window and `Ctrl+,` the preferences —
  both the key every application on this machine already answers to, and
  the Mac shell puts Settings on the application menu where a Mac looks
  for it. The panel's width and the toolbar's position stay on their own
  keys and out of all this: they are written on every frame of a drag,
  and a JSON blob rewritten sixty times a second to remember a drag is
  not a preference, it is a leak.

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
  editing ✅ (the node tool; handles per anchor); gradient fills ✅ — both
  said "pending" here long after they landed, found by the October 2026
  audit.
- Raster: place PNG/JPEG as RasterObject ✅ (content-addressed resource pool,
  pixels embedded as PNGs in .chitra, undoable placement, move + hit test);
  scale/rotate ✅ (the same handles and rotation knob every layer has).
- Layer panel: hide ✅, select ✅, delete ✅, reorder ✅ (MoveNode command:
  reorder + reparent with subtree-cycle protection), opacity slider ✅,
  blend-mode picker ✅; grouping UI ✅ (Layer › Group and Ungroup, and
  their keys).
- Selection ✅ (hit test + move tool with live preview); corner resize
  handles ✅ (anchored scaling); rotation handles ✅ (the rotation knob).

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
  Gradients ✅ (a fill can be a gradient, with its stops on the panel and
  its ends as handles on the canvas; this line said "pending" long after
  it landed). Layer rename ✅ (SetName command, inline edit).
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
  holds all of it. Below 640px — a phone's own width — the tool rail goes
  along the bottom ✅, where the hand already is: a column down the left
  edge takes width from a canvas that has none to spare and puts every
  tool at the far end of the reach, while along the bottom it takes
  height, which a page held upright has more of. The same buttons in the
  same order, the rail scrolling sideways rather than the page, the
  shape flyout opening upward since below the rail is off the screen, and
  no grip to carry it off by since down there it has one place to be.
  Reversed rather than reordered, so it is still the first thing a
  keyboard or a reader arrives at.
  Measuring the phone layout to decide what to do next is what turned up
  the defect that mattered: the top bar is one row tall by a *fixed*
  height, which is right while nothing wraps and wrong the moment
  something does. Below 900px it is told to wrap, and what it wrapped
  went on overflowing below the bar, over the canvas and under the ruler
  — visible and unpressable. The layers button is the last thing in the
  bar and so the first to wrap, which made the one way to the layers on a
  phone the thing that broke. The bar grows to hold its rows now, and the
  suite presses that button at a phone's width rather than only at a
  tablet's, where nothing had wrapped and nothing was wrong. It was worse
  than the first reading of it: the document's name and the zoom were
  under the ruler too, and it began at 640px — a tablet held upright, not
  only a phone.
  Which is a *kind* of defect worth a question of its own, since it is
  invisible to every other sort of test: a control that is there, the
  right size, in the right place, and covered. Nothing about the markup is
  wrong, nothing about the layout says so, and it looks right in a
  screenshot. What says so is asking the page what is under the middle of
  each control, which is what a finger would find — so the suite walks
  every button, field and picker at four widths and asks exactly that.
  With the bar's fix taken out it names three controls at 360 and 500,
  one at 640, and stays quiet from 720 up, which is the defect's own
  shape.
- Touch targets ✅: the stylesheet had grown the handles on the *artwork*
  under a coarse pointer and left the chrome at its mouse sizes. A finger
  covers about a centimetre and cannot see under itself, so what a mouse
  hits at five pixels a thumb cannot hit at all — and five pixels is
  exactly what the corner that opens a shared slot was, which on a phone
  left the ellipse, the polygon, the star and half the ways of selecting
  with no way to reach them. The triangle stays where it was, drawn by a
  mark of its own, and the press grows around it: eighteen on a
  forty-four slot, so the middle of the tool is still the tool's, since a
  corner big enough to swallow the button it sits on is a different bug.
  The bar's own — menu labels, chrome buttons, the ink swatch, the layers
  button — take forty; its two fields take thirty-six, because a field
  has to be got into before it can be typed in; a palette chip takes
  twenty-eight. Twenty-five controls were under forty-four on a phone;
  what is left is the two fields, the two corners and the tools' own
  thirty-eight. The suite asks this on a page of its own, since whether a
  pointer is coarse is decided when the context is made and not by
  resizing one, and the reading that matters is behavioural: tap the
  corner and the rest of the shapes come out, while the middle of the
  slot still answers as the tool.
- Menus stay inside the window ✅, and what is past a menu's end can be
  got to: a menu opens under its label and is as tall as it has items, so
  one taller than the room under it went on past the bottom of the window
  with nothing to scroll — on a phone held sideways, fourteen of Edit's
  twenty-four items were off the screen, File five of fourteen, View six
  of fifteen. They scroll now, against a height that leaves room for the
  bar even when it has wrapped to three rows: generous rather than exact,
  since what matters is that a menu can never run out of the window and
  one that scrolls sooner than it had to is no worse than one that fits.
  And on a narrow window they open from the bar's left edge rather than
  from under their own label, because a popup is fifteen rem wide and
  there is no room for that under a label near the right of the bar —
  View hung three pixels off the edge at a phone's width. Given the room
  back they go under their own labels again, which the suite checks so
  the narrow rule cannot leak.
- Dialogs stay inside the window ✅: one is centred on its scrim, so one
  taller than the screen hangs off *both* ends and takes its buttons with
  it. On a phone held sideways that left New document 394 pixels tall on
  a 320-pixel screen with no Create and no Cancel anywhere on it, and
  nothing to scroll — the only way out was the Escape key, which a phone
  does not have. A dialog has a size it cannot exceed now and, past that,
  its own scrollbar, with the row that answers it kept against its
  bottom so what closes it is where it always is rather than down the end
  of a list of fields.
- A phone's own bar ✅: on a phone — under 640 wide, or under 500 tall,
  which is a phone held sideways and not a window anyone drags that
  small — the menubar's eight labels became one Menus button that opens
  the menus as a drawer from the left edge: each menu a row a thumb can
  hit, opened in place, its sub-menus folding open under it, and the
  shortcut hints left off under a coarse pointer since there is no
  keyboard to press them on. The bar is one row again, keeping the
  document's name, the zoom, undo and the layers; the rulers go, since
  forty pixels of width and of height is a lot of a phone to give to
  numbers nobody aims by with a thumb. Held sideways, the rail down the
  left had sixteen tools in 339 pixels and no way to the last six —
  Zoom among them; it scrolls now. Smoke 11a holds it upright and
  sideways on a touch page; 9s and 9u, which opened New document and the
  long menus through the menubar on a small window, open them through
  the drawer where there is no menubar.
- A hand on glass ✅ — what a mouse has words for and a finger did not.
  Two fingers tapped is undo and three is redo, the gesture every
  drawing app on a tablet has taught; a tap is every finger down and up
  within 350ms with none travelling more than twelve pixels, so a pinch
  and a hold stay what they were. A finger held still on the page for
  450ms opens the canvas's menu, the one a right-click opens, and picks
  what it is held on first; whatever the finger had begun is let go of,
  and it is ignored until it lifts, so lifting it draws nothing — not
  while painting or placing a path's anchors, where holding still is the
  work. Android calls the same press a context menu a little later, and
  the two are one. A pen landing takes the page from the palm that
  usually gets there first, and lets go of what the palm began; a finger
  landing while a pen is down is not heard at all. And a touch screen
  has Shift, Alt and Ctrl of its own at the canvas's corner: tapped, a
  key stays down until tapped again; held while the other hand works, it
  lets go with the finger. It is one listener in the window's capture
  phase that makes every pointer event *say* the key is down, ahead of
  every handler, so the canvas, the handles and the layer list's
  shift-click all answer to it without one of them knowing it is there.
  `src/touch.ts` holds the bookkeeping, unit-tested without a browser;
  smoke 11b drives all of it on a tablet's page. Inside the shell on a
  phone or a tablet the menus stay in the window rather than being
  handed to a system menu bar there is none of. Writing the test for the
  on-screen Shift turned up a defect on any short window: the layers
  panel did not scroll, so with a layer's properties open above it the
  list itself was past the bottom of a 768-high screen — a tablet on its
  side — and could not be reached. It scrolls now.
- The shell compiles for a phone ✅ (`cargo clippy --target
  aarch64-linux-android`, run here against the NDK, and the iOS one on
  the Mac runner; both are CI jobs now). It found two things a desktop
  build never asks: the dialog plugin has no folder picker on mobile, so
  a many-file export says the device has no folders rather than failing
  to build; and the build script read `cfg(target_os)` — which in a build
  script is the machine building, not the one built for — so a Mac
  building for an iPhone would have compiled the Vision Swift for macOS
  into the iOS link. It reads `CARGO_CFG_TARGET_OS` now. An installable
  Android build ✅: CI's `android-apk` job runs `tauri android init`
  where the runner has the SDK and builds an APK for arm64, kept as
  an artifact — a release build, optimised, signed with a key made on the
  runner and thrown away with it (the debug one was 135 MB and ran the
  engine unoptimised); each run's key is new, so one installs over
  another only once the old is removed (it took starting the CLI through `npm run tauri`, from
  `shells/tauri/package.json`, since the Gradle build calls the CLI
  back the way it was started) — the Android project is generated from tauri.conf.json
  each time rather than committed, so it has nothing in it to keep in
  step by hand. Still to do: a store build, signed with a key that is
  kept — the owner's to make and hold as a CI secret;
  and iOS past compiling, which wants a Mac with Xcode and a team to
  sign for.
- The layers as a sheet ✅: on a phone held upright the panel came
  across from the side and covered six tenths of a 390-wide screen, the
  picture with it. It comes up from the bottom edge now, half the
  height, so the page stays in sight above what is being changed about
  it; its grip, pulled up, takes it to nearly all of the screen, and
  pulled down lowers it a step at a time — tall to half, half to away —
  while a tap toggles between the two heights and a pull too short to
  mean it does nothing. Held sideways it is the side overlay still,
  since there the width is what is spare. Smoke 11d.
- A phone's browser ✅. The page was `100vh` tall, which a phone's
  browser counts to include the strip behind its own toolbar — so the
  tool rail along the bottom sat partly under it; it is the height that
  shows now (`100dvh`). A drag down from the top of the page was
  Android Chrome's pull-to-refresh, which throws away whatever has not
  been saved; it is not any more. The viewport runs under the notch
  (`viewport-fit=cover`), which is what makes the safe-area padding the
  stylesheet already had mean anything, and the editor keeps clear of
  the notch on a phone held either way. The on-screen keyboard makes the
  page shorter (`interactive-widget=resizes-content`) rather than
  covering its lower half, where text being typed on the canvas often
  is — Android's Chrome had only been shrinking the visual viewport.
  Safari ignores the setting; it scrolls the focused field into view
  on its own. And it can be added to a home
  screen and opens as an app: a manifest, the shell's own icons, and the
  browser's bar taking the app's colour, following the theme. Smoke 11e.
- Offline ✅: put on a home screen it is an app, and an app opens with
  no signal. A service worker (`app/public/sw.js`, registered by
  `src/offline.ts` in a built page in a browser — not the shell, whose
  files are on the machine, nor the dev server) keeps what a visit
  loads. Files under assets/ are named by their contents, so a kept one
  answers first; the page, the manifest and the fonts go to the network
  first, so a new version is never held back by an old copy, and the
  kept one answers only when the network does not. A worker does not
  serve the visit that installed it, so the page tells it what that
  visit loaded once it is in charge; kept assets the page no longer
  loads go then, so the cache does not grow by a build on every update.
  Smoke 11f reloads with the network off and draws a rect. It failed
  once on a loaded machine, here and on CI alike, with seven files kept
  and not the engine: the page said what it had loaded on its `load`
  event, and the engine — seven megabytes the script fetches — finishes
  after that as often as not. It says so again whenever more finishes
  loading now (a `PerformanceObserver`), so the engine is kept however
  late it arrives.
- Pictures given to the app ✅. Installed, it is in the phone's own
  share sheet: "Share → Chitrakar" from the photos app posts the picture
  to `share-target` (`share_target` in the manifest), which no page can
  receive — the service worker takes it, keeps the files, and opens the
  page with `?shared`; the page places them as it places a drop
  (`takeFiles`, the one way in for a drop, a share and a launch) and puts
  the address back so a reload takes nothing twice. And it can be the
  app that opens a `.chitra` or a picture from the system's files
  (`file_handlers`, read through the launch queue). Writing the test
  turned up the suite's own server answering 404 for "/?shared=1" — it
  stripped the query after deciding whether the path was the root — and
  the worker now falls back to the kept page for any navigation a host
  has no page for. Smoke 11g.
- The rail's options lie down with it ✅. Along a phone's bottom edge
  the rail carried what it carries in a column — the palette two
  swatches wide and as tall as it had swatches, and the paint tool's
  width and edge stretched to match — which made it 133 pixels tall on a
  390 screen, and put the width and the edge at the far end of its
  scroll, out of sight. In a row it is as tall as a tool (60), and
  picking a paint tool brings its width and edge into sight. Smoke 11h.
- A finger that moves the page once a pen is in hand ✅. On a tablet
  with a pen the pen draws and the hand moves the page — every drawing
  app has taught it, and it is what makes a finger resting on the glass
  harmless rather than a mark. By default one finger draws, as a mouse
  does, until a pen has touched the screen, and moves the page from
  then on; `prefs.finger` (Preferences › General › One finger) says
  always draw or always move the page instead. Two fingers move it
  whatever it says. A finger that moves the page is a pan like the
  hand tool's, so taps still undo and a long press, which belongs to a
  finger that draws, does not open the menu. A long press that opens the
  menu ticks under the finger (`navigator.vibrate`), as a phone's own
  does. Smoke 11i; 11b's palm
  rejection now runs with fingers set to draw, which is where it
  matters.
- The layer list under a finger ✅. Its rows had been set to give the
  browser nothing (`touch-action: none`) so that dragging one reordered
  the layers — which meant a swipe on the list, which is mostly rows,
  reordered the layers too and never scrolled it. On a phone with more
  layers than the sheet could show, there was no way down to the rest.
  A swipe scrolls now (`pan-y`), and a row held still for a third of a
  second lifts — a tick under the finger, the row dimmed — and is
  carried from then on, the scroll declined as it begins (a
  non-passive `touchmove` on the list). A mouse or a pen drags at once,
  as before. Smoke 11j sends real touches through the browser's own
  input, since a synthetic event never meets its scrolling, which is
  the whole question.
- A photograph's page, held in what a phone can spare ✅. In the browser
  an engine's memory, once grown, is never handed back, so a peak is not
  a moment — it is what the tab weighs for the rest of the visit. A
  12-megapixel page (4032 × 3024) was measured three ways and found
  three peaks far above what was kept:
  - **The first frame of a new document** was drawn before the document
    had been given its view, so it was the whole page at full size: a
    195 MB float surface and a 48 MB frame, for an *empty* page. The
    session is given its fitted view before its first frame now
    (`fitSession`, in New and in Open). An empty 12-megapixel page went
    from about 270 MB to 7 (smoke 11k holds it under 60).
  - **Reading a photograph** decoded it as RGB and copied it into RGBA,
    36 MB and 48 MB alive at once. An 8-bit picture is read straight
    into the RGBA buffer and widened in place, back to front
    (`eight_bit`): the decode peaks at what it keeps.
  - **Saving** cloned the whole document — every picture's pixels — to
    write a manifest that never writes pixels; and the recover draft
    saves on its own every little while. The manifest borrows the
    document now (`ManifestOut`).
  Natively, opening the photograph on a phone-sized view went from a
  129 MB peak to 95, and saving from 149 to 102: the photograph's 48 MB
  and a screen's float cache, and the file being written.
  `core/codecs/tests/peak.rs` counts allocations in its own process and
  holds reading and saving to one copy of the pixels (with either fix
  taken out it fails, at 13 MB and 8 MB for a 7 MB picture).
  **The frame ✅.** Moving that photograph on a phone-sized view
  repainted in about 95 ms natively, single-threaded — several hundred
  in a phone's WebAssembly. Half the resolution while a finger moves
  was measured and did not help (78 ms): halve the pixels and each
  one's footprint in the photograph doubles, so it averages four times
  the texels (`picture_footprint`), and every frame still read all
  twelve million. The cost was the source, not the screen, so the
  answer is a pyramid (`core/render/src/pyramid.rs`): the picture kept
  again at a half, a quarter, … of its size, each level the last
  averaged two by two in premultiplied linear light, sixteen bits a
  channel. Shrunk to a half or more a picture is read from the level
  whose texels a device pixel covers between one and two of
  (`picture_level`), and averaged over that level's footprint with the
  same taps as before; its outline is still the picture's own. The 12
  MP photograph fitted to a 1000 × 750 view went from 80 ms a frame to
  21, and to 400 × 300 from 39 to 4; the first frame pays about 140 ms
  once to make the levels. They are kept for the whole process (the
  desktop shell's GPU commands run on a pool of workers), by the
  picture's id, least recently used first past 96 MB — a 12 MP
  photograph's levels are 32 MB — and checked against a fingerprint of
  the picture before they are handed out, since an id read out of a
  file is the file's word for it. The GPU backend uploads the level the
  CPU reads and scales the box's coordinates by how much of the level
  the picture spans, which an odd size makes less than all of it.
  **A picture in perspective ✅** was worse: 450 ms a frame for the same
  photograph with its corners moved, since there every pixel took up to
  eight by eight taps of the photograph itself and asked whether all
  four of its corners were inside the quad. It is 92 ms now. Each pixel
  reads its own level, since perspective shrinks the far end more than
  the near, and is mixed between the two levels its footprint falls
  between — the photograph itself counting as the first — because a
  level read one or the other is a seam across the road where one gives
  way to the next. Which pixels are wholly inside the quad is worked out
  once a row, as an interval, rather than four corners a pixel. The GPU
  still hands a picture in perspective back, so there is no parity to
  keep.
- Touch + Apple Pencil/stylus input (pressure into the input pipeline early, ahead of
  brush tools). Pressure ✅ (a pen's own reading drives the brush's width,
  with a mouse's speed standing in for it); the view's own gestures ✅
  (two fingers pinch to zoom and carry the page, and take the gesture
  over from the one that began it).
- Platform file integration (Files app, Android SAF, share sheets).
  Share sheets ✅: where the device has one, the export window has a
  Share… beside Export — the same bytes, handed to the system, which on a
  phone is where a picture goes next and Save to Files is one of its
  answers. The set goes as three files in one sheet; a sheet put away
  leaves the window up, as a cancelled save panel does; a format the
  sheet will not take (a PDF, on a sheet that takes pictures) is said
  rather than swallowed. And the phone shell, asked for several files
  with no folders to choose, shares them. Android's own storage ✅ in
  the shell: its picker hands over `content://` addresses, not paths,
  which the shell had been refusing ("not a file path"). They are kept
  as they came and opened, read and written through the fs plugin, which
  asks the system; written in place, since there is no file beside one
  to write first. The page names them from the path encoded in their
  last segment, so a document opened from Downloads is called by its
  name. `e2e/files.test.mjs` holds the naming, the shell's own test the
  address against the path; smoke 11c the share. On an iPhone or an iPad a
  file picked in the system's panel is the app's to read only while it
  holds the grant the picker gave with it, which the fs plugin takes
  when it opens a `file://` address; the shell had been turning those
  into bare paths, which iOS refuses. It keeps them as addresses there
  (`files::chosen_on`, tested on any machine with the platform's answer
  said outright), and the page names them decoded.

### Phase 5 — Depth (ongoing)
- Pen tool + full path editing; boolean operations on shapes ✅. The pen
  draws straight and smooth segments and closes a path; an anchor can be
  put onto a line where it already runs and taken off again
  (`Session::insert_anchor` / `remove_anchor`,
  `adding_an_anchor_leaves_the_path_where_it_was`,
  `an_anchor_on_a_brushed_line_keeps_the_widths_lined_up`); and union,
  difference and intersection combine two shapes into one layer
  (`Session::boolean_nodes` over `chitrakar_render::boolean`,
  `booleans_combine_two_shapes_into_one_layer`,
  `shapes_that_share_an_edge_still_combine`,
  `combining_shapes_keeps_the_shape_that_was_combined`). The same
  booleans are what add, subtract and intersect a *selection*.
  This line carried no tick for a long time after the work was done,
  which is worth a word: a roadmap that under-reports is not harmless.
  A fresh session reads it as work outstanding and either redoes it or
  goes looking for what is missing, and both cost more than the tick.
- Text objects ✅ first pass: live TextSpec nodes (string, size, color as
  document state; glyphs rasterize at render time via ab_glyph + bundled
  DejaVu Sans, kerned per-glyph layout with newline support), blitted through
  the node transform with mask/opacity/blend support; Text tool click-places,
  panel edits content/size/color with gesture preview; resize handles work.
  Proper shaping, font choice and weights ✅, all three, and this line
  said "pending" long after they landed. The shaping is `rustybuzz`, and
  it is the font's say over what the text becomes rather than a walk
  along the characters: `fi` is one glyph, `office` is four, `AV` sets
  closer than its advances add up to, and `e` plus a combining acute is
  the same single glyph as the precomposed `é`
  (`the_font_shapes_the_line_rather_than_the_characters`, which catches a
  shaper put back to one glyph per codepoint — checked by doing exactly
  that). A block names its face, and a weight or a lean the face cannot
  supply is synthesized rather than dropped: an oblique twin is used
  where one is registered and a lean or a thickening is applied where it
  is not (`italic_leans_the_glyphs_when_no_oblique_face_exists`,
  `bold_thickens_the_glyphs_when_no_bold_face_exists`). `parley` is not
  used and is not wanted: line breaking, alignment and wrapping are
  `text::set`'s own and are tested there.
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
  children with layers of its own. Live in SVG and PDF. Standing in for a layer deeper than the
  original's own children ✅: a stand-in for a *group* is a copy of that
  group rather than a copy of its contents, so standing in composes — one
  call a level — and everything not stood in for goes on following the
  original. A card holds a button holds a label; change one card's label
  and that card still follows the original for the button's chrome and
  the card's own edge, live. Before, a stand-in was a deep copy of
  whatever it replaced, so reaching the label meant detaching the whole
  button: the second call did not fail gracefully, it failed with "that
  layer is not a copy", because what came back was not one
  (`a_copy_can_differ_at_a_layer_deeper_than_the_originals_own`). The
  browser suite builds that card through the UI — two groupings, a copy,
  a stand-in for the button, then one for the label inside it — and
  reads the answer off the canvas, so the panel's own way in is covered
  and not only the engine's. Leaves
  are still copied by value, since what is wanted there is a shape to
  edit rather than a picture of one; and only a plain group can be stood
  in for, which is the same rule as everywhere else in this feature.
  A stand-in names the *layer* it stands in for rather than where that
  layer sat ✅. A position is true only until the original's layers are
  added to, reordered or thinned out, and then it quietly means a
  different layer: put a layer at the front of a component and every
  copy's stand-in slid onto its neighbour — the copy stopped drawing one
  of the original's layers and started overriding another, with nothing
  to say so and the file written that way too. It is a `NodeId` now, so
  adding, shuffling and deleting simply work: the copy draws its own
  layer where the original now holds the one it replaced, and a layer
  whose original has gone is kept and drawn after the rest rather than
  lost, with undo putting it back in its place
  (`a_stand_in_holds_to_its_layer_when_the_originals_are_shuffled`).
  Both ways of saying it are lists of numbers in a file, so nothing in
  the bytes says which one a file meant: the container's format version
  does, and a file written the old way is carried over on the way in and
  opens as the page it was
  (`a_file_that_said_where_a_stand_in_sat_opens_as_the_page_it_was`) —
  which is what `FORMAT_VERSION` 2 is.
  What is inside a group changes only what is inside it, and a copy with
  a stand-in keeps that ✅. A group holding an adjustment — or a filter, a
  clone layer, a layer with a blend — is drawn on a surface of its own,
  so what is in it reaches its neighbours and nothing beneath. A copy
  drew the group and inherited that; a copy *standing in* for one of the
  group's layers draws a list of layers instead, and the list went
  straight onto the page. So standing in for a layer of one copy sent the
  original's own adjustment over the whole page — the ground, the
  original and everything else on it, two stops down
  (`an_adjustment_in_a_copied_group_stays_inside_the_copy`). The list is
  asked the same question the group is now
  (`chitrakar_render::any_reads_backdrop`) and isolated on the same terms.
  The GPU backend had it too and hands such a page back rather than
  drawing a different picture
  (`a_copy_standing_in_where_the_group_reads_the_backdrop_goes_back`);
  PDF was already right by the route it takes, since a copy holding
  anything not live goes over as pixels, and SVG omits adjustments
  wherever they are and says so in the markup.
- **What the two windows do not do yet.** The export window wrote
  through the browser's download even in the desktop shell; it asks the
  system's save panel there now, and a folder for a set ✅ (§0 *Desktop
  shell, files by path*). The export
  preview is in ✅ (the file itself, decoded, beside the settings; a
  PDF or a TIFF shows the page standing in), and so is the `@1x/@2x/@3x`
  set ✅ (three files in one press), exports kept by name ✅ (chips above
  the formats), and slice export ✅ (a file per region kept by name). The preferences window
  lets the *tool* keys be rebound ✅ (the Tools page; see §0 *Chrome*) and
  the command keys are
  the person's too ✅ (the Keys page; see §0 *Chrome*). A light theme is in ✅ (see §0
  *Chrome*: every colour of the chrome is a token on the root, the
  light set is one attribute away, and `prefs.theme` is the system's
  choice or one of the two outright). Before that the stylesheet
  was `color-scheme: dark` and
  single-palette throughout, so a light mode is a token pass over
  ~2,000 lines of CSS rather than a switch, and worth doing as its own
  piece of work.
- Live effects ✅ (drop shadow, inner shadow, outline — on a layer, a
  group, a copy, drawn by both renderers and carried to SVG and PDF).
  Copy and paste of a layer's look ✅; named styles ✅ (`Session::keep_style`,
  `apply_style`; chips in the rail's palette and rows on the Layer menu's
  Styles submenu).
- **Reviewed against Canva, Affinity and Photoshop (September 2026):**
  the menus and the rail are theirs in shape now — see §0 *Chrome* —
  and what each of them has that this does not is listed, in the order
  it would pay, in §0 "Next up" item 4: a Help menu, a gradient tool, a
  fill tool, eraser and heal as tools of their own, a node tool, scaling
  the whole design, open recent, named styles, brush presets, a shape
  library. That list is the roadmap for the chrome; nothing on it is a
  new kind of layer, which is the measure of how far the document model
  already reaches.
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
