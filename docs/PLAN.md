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
  What is still left out, each for a stated reason: a `filter`, so a
  blurred element comes in sharp — there is no way yet to say "blur this
  one layer", a filter layer here reaching everything below it and a
  clip to the shape cutting the very spread that makes it a blur; a mask
  with real grey in it, which wants a raster mask and the pixels pooled
  for it; a pattern fill, which wants a tile rasterized; and
  `stroke-dashoffset`, which has no field to land in, so a broken line
  starts its pattern at the beginning. Every one of those is a layer
  that arrives plainer than the file, never one that arrives missing.
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
- **Verify before committing:** `cargo test --workspace` (~486),
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all`,
  and in `app/`: `npm run build && npm run test:e2e` (~1138 browser
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
     *holds* rather than the kind of layer it is: press ink, a healing
     stroke, an outline wider than a pass will walk, an effect on a clone
     layer, and a stroke carrying a region on a layer whose own mask is
     already riding that slot.
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
     something to read. What still goes back is a healing stroke, whose
     shift is an average over the whole stroke before any of it goes
     down: a reduction, and a pass of quads is not where one happens.
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
     the pixels-over-the-boundary shape all at once. Until one of those
     is chosen the backend is a validated second opinion and nothing
     else, which is a fine thing to be — holding it against the CPU
     renderer has turned up real defects on both sides, the clone
     stroke's lost region among them — but it is not on screen. The
     fixture audit
     (`whatever_the_gpu_agrees_to_draw_it_draws_the_way_the_cpu_does`) is
     what to run while doing any of it. See docs/spikes/gpu-rendering.md.
  2. Mobile shells: `tauri android init` / `ios init` (needs SDKs, so it
     wants a machine with Xcode/Android Studio).
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
     fixture's twenty-six layers can be removed outright without moving
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
     A `filter` is dropped, so a blurred element comes in sharp — at two
     pixels outside a blurred rect resvg draws a quarter coverage and
     this draws nothing. That one is not an importer gap but a model one:
     a filter layer here applies to everything below it, and clipping it
     to the shape would cut exactly the spread that makes it a blur, so
     there is no way to say "blur this one layer" yet. And a pattern fill
     comes in unpainted, since `Paint::Pattern` has no answer here; the
     honest fix is to rasterize a tile, which is worth less than it costs
     for the files people bring to a photo editor.
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
  `PreferencesDialog.tsx` is a rail of six groups, which is Photoshop's
  shape and Affinity's both, and earns the furniture at six — a flat
  list of eighteen controls is a search problem. What is deliberately
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
- Touch + Apple Pencil/stylus input (pressure into the input pipeline early, ahead of
  brush tools). Pressure ✅ (a pen's own reading drives the brush's width,
  with a mouse's speed standing in for it); the view's own gestures ✅
  (two fingers pinch to zoom and carry the page, and take the gesture
  over from the one that began it).
- Platform file integration (Files app, Android SAF, share sheets).

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
- **What the two windows do not do yet.** The export window writes
  through the browser's download, so the desktop shell gets no native
  save panel and no choice of folder — Tauri's dialog plugin is already
  a dependency, so this is plumbing rather than a decision. There is no
  export *preview* (Affinity and Photoshop both show the picture beside
  the settings, and a JPEG at quality 5 is a thing you want to see
  before you take it), no batch or slice export (several sizes in one
  press, the `@1x/@2x/@3x` set an asset pipeline wants), and no
  remembering more than one export setup. The preferences window has no
  keyboard-shortcut editor — the keys are still literals in the keydown
  handler — and no theme: the stylesheet is `color-scheme: dark` and
  single-palette throughout, so a light mode is a token pass over
  ~2,000 lines of CSS rather than a switch, and worth doing as its own
  piece of work.
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
