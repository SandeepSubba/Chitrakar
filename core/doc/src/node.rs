//! Node types of the scene graph. Every node is a live object described by
//! parameters — nothing here ever stores baked pixels except the immutable
//! source resource a [`RasterRef`] points at.

use crate::NodeId;
use chitrakar_color::AuthoredColor;
use serde::{Deserialize, Serialize};

/// How a layer's colour meets what is under it. The names and the
/// arithmetic are the W3C compositing spec's, which is what SVG's
/// `mix-blend-mode` and PDF's `/BM` say too — so a page blends the same
/// way in the engine and in what it exports.
///
/// Additive: the three that existed keep their names, and a document
/// written before the rest still reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    /// Multiply where the backdrop is dark and screen where it is light:
    /// contrast, judged by what is underneath.
    Overlay,
    Darken,
    Lighten,
    /// Brightens the backdrop towards the layer's colour.
    ColorDodge,
    /// Darkens it towards the layer's colour.
    ColorBurn,
    /// Overlay with the two swapped: contrast judged by the layer.
    HardLight,
    /// A gentler hard light, as if the layer were a diffused lamp.
    SoftLight,
    Difference,
    /// A softer difference, with no black at the middle.
    Exclusion,
    /// The four that take one part of a colour and leave the rest: the
    /// layer's hue on the backdrop's brightness, and so on.
    Hue,
    Saturation,
    Color,
    Luminosity,
}

/// 2D affine transform (row-major 2×3: `[a c e; b d f]` maps column vector
/// `(x, y, 1)`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub e: f32,
    pub f: f32,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: 0.0,
            f: 0.0,
        }
    }
}

impl Transform {
    /// `self` applied after `inner`: the inner transform runs first, then
    /// this one. That is what nesting means, so it is how a group's
    /// transform reaches its children — and how ungrouping folds the
    /// group's transform into them.
    pub fn compose(self, inner: Transform) -> Transform {
        Transform {
            a: self.a * inner.a + self.c * inner.b,
            b: self.b * inner.a + self.d * inner.b,
            c: self.a * inner.c + self.c * inner.d,
            d: self.b * inner.c + self.d * inner.d,
            e: self.a * inner.e + self.c * inner.f + self.e,
            f: self.b * inner.e + self.d * inner.f + self.f,
        }
    }

    pub fn translation(x: f32, y: f32) -> Self {
        Self {
            e: x,
            f: y,
            ..Default::default()
        }
    }
}

/// One stop on a colour ramp: where it sits along the gradient (0..=1) and
/// the colour there. Stops are authored colours like any other fill, so a
/// CMYK document's gradients resolve through its press profile too.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GradientStop {
    pub offset: f32,
    pub color: AuthoredColor,
}

/// A gradient fill. Geometry is in the shape's own local bounding box
/// normalized to 0..1 on each axis (SVG's objectBoundingBox units), so a
/// gradient follows its shape when the shape moves or is resized without
/// needing its own transform to be kept in step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Gradient {
    /// Ramp along the line from `from` to `to`, clamped past either end.
    Linear {
        from: [f32; 2],
        to: [f32; 2],
        stops: Vec<GradientStop>,
    },
    /// Ramp outward from `center` to `radius` (in units of the box's
    /// half-diagonal), clamped past the edge.
    Radial {
        center: [f32; 2],
        radius: f32,
        stops: Vec<GradientStop>,
    },
}

impl Gradient {
    pub fn stops(&self) -> &[GradientStop] {
        match self {
            Gradient::Linear { stops, .. } | Gradient::Radial { stops, .. } => stops,
        }
    }
}

/// One stroke of paint: the line it was drawn along in the layer's own
/// space, the brush radius at each of its points, the colour it lays
/// down, how far in from the rim its edge fades, and whether it paints
/// or takes paint away.
///
/// The region it covers is the union of round-capped segments between
/// consecutive points — the same shape a stroked path covers — with the
/// coverage falling off across the soft edge instead of stopping dead.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaintStroke {
    pub points: Vec<[f32; 2]>,
    /// Radius at each point. A shorter list repeats its last entry, so a
    /// stroke of one radius needs only one.
    pub radii: Vec<f32>,
    pub color: AuthoredColor,
    /// How much of the radius the edge fades over: 0 is a hard edge and
    /// 1 a brush with no solid core at all. Additive, so a stroke
    /// written before softness existed reads as a hard one.
    #[serde(default)]
    pub softness: f32,
    /// Takes paint off the layer instead of laying it down.
    #[serde(default)]
    pub erase: bool,
    /// Where a clone stroke takes its pixels from, as an offset in the
    /// layer's own space: the point it paints at plus this is the point
    /// it reads. Only a clone layer reads it — a brush lays its own
    /// colour and has nowhere to read from — and it is additive, so a
    /// stroke written before cloning existed reads as no offset at all.
    #[serde(default)]
    pub source: [f32; 2],
    /// Heal rather than clone: lay down the texture from the source but
    /// the colour of the place it lands in, so a patch lifted from
    /// somewhere lighter or darker still sits into its surroundings.
    /// Only a clone layer reads it, and it is additive.
    #[serde(default)]
    pub heal: bool,
}

impl PaintStroke {
    /// The brush radius at point `i`, which is the last one given for
    /// every point past the end of the list.
    pub fn radius(&self, i: usize) -> f32 {
        match self.radii.len() {
            0 => 0.0,
            n => self.radii[i.min(n - 1)],
        }
    }

    /// The box the stroke covers in the layer's own space, or nothing
    /// when it covers none.
    pub fn bounds(&self) -> Option<[f32; 4]> {
        self.bounds_from(0)
    }

    /// The box the stroke covers from point `from` onward — what a
    /// stroke still being drawn adds each time it grows.
    pub fn bounds_from(&self, from: usize) -> Option<[f32; 4]> {
        let mut box_: Option<[f32; 4]> = None;
        for (i, p) in self.points.iter().enumerate().skip(from) {
            let r = self.radius(i).max(0.0);
            let b = [p[0] - r, p[1] - r, p[0] + r, p[1] + r];
            box_ = Some(match box_ {
                None => b,
                Some(a) => [
                    a[0].min(b[0]),
                    a[1].min(b[1]),
                    a[2].max(b[2]),
                    a[3].max(b[3]),
                ],
            });
        }
        box_.filter(|b| b[2] > b[0] && b[3] > b[1])
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum VectorShape {
    Rect {
        width: f32,
        height: f32,
        /// Corner radius, clamped at render time to half the shorter side.
        /// Additive: a document written before rounded corners existed
        /// loads with square ones, which is what it had.
        #[serde(default)]
        radius: f32,
    },
    Ellipse {
        rx: f32,
        ry: f32,
    },
    /// Anchor polyline. With `smooth`, rendering interpolates a Catmull-Rom
    /// spline through the anchors instead of straight segments.
    Path {
        points: Vec<[f32; 2]>,
        closed: bool,
        #[serde(default)]
        smooth: bool,
        /// Per-anchor bezier control offsets, `[in_x, in_y, out_x, out_y]`
        /// relative to the anchor. Empty means none, which is why old files
        /// keep loading; when present they are explicit and override
        /// `smooth`, whose curve is inferred rather than authored.
        #[serde(default)]
        handles: Vec<[f32; 4]>,
        /// Extra closed rings drawn with the same fill. Coverage is
        /// even-odd across all of them, so a ring inside another cuts a
        /// hole and a ring beside it is a second island — which is what
        /// makes a compound path, and what a boolean operation produces.
        /// Straight-sided: only the main ring carries curves. Additive.
        #[serde(default)]
        subpaths: Vec<Vec<[f32; 2]>>,
    },
}

/// Reference to an immutable pixel resource embedded in the document
/// (`resources/` in the .chitra container). Content-addressed so identical
/// placements share bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RasterRef {
    pub resource_id: String,
    pub width: u32,
    pub height: u32,
}

/// Non-destructive adjustments; applied at render time to everything below
/// the layer (or to the object they're attached to).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Adjustment {
    BrightnessContrast {
        brightness: f32,
        contrast: f32,
    },
    Exposure {
        stops: f32,
    },
    HueSaturation {
        hue_degrees: f32,
        saturation: f32,
        lightness: f32,
    },
    /// Levels, in linear light like every adjustment here: the input
    /// range `in_black..in_white` is stretched to 0..1, a gamma lifts or
    /// sinks the midtones (above 1 lightens), and the result lands in
    /// `out_black..out_white`.
    Levels {
        in_black: f32,
        in_white: f32,
        gamma: f32,
        out_black: f32,
        out_white: f32,
    },
    /// A tone curve through `points` (input, output), both 0..1 in the
    /// display encoding — the way every photo editor draws one, so the
    /// middle of the graph is a middle grey rather than a linear quarter.
    /// The renderer sorts the points, interpolates a monotone cubic
    /// through them and holds the curve flat past the first and last, so
    /// two points on the diagonal (or fewer) is the identity.
    Curves {
        points: Vec<[f32; 2]>,
        /// A curve of its own for each channel, run after the master one
        /// — what colour grading is done with: lift the blue in the
        /// shadows, pull the red out of the highlights. Empty is the
        /// identity, which is why a document written before they existed
        /// still looks like itself.
        #[serde(default)]
        red: Vec<[f32; 2]>,
        #[serde(default)]
        green: Vec<[f32; 2]>,
        #[serde(default)]
        blue: Vec<[f32; 2]>,
    },
    /// White balance. `temperature` warms the picture as it rises and
    /// cools it as it falls (-1..=1, blue against amber); `tint` runs the
    /// other way a light source strays, green against magenta. Both act
    /// on the linear channels, which is where a light's colour actually
    /// lives.
    WhiteBalance {
        temperature: f32,
        tint: f32,
    },
    /// Saturation that leaves the already-vivid alone: the further a
    /// colour is from grey, the less of the change it takes. That is
    /// what keeps skin from going orange while a dull sky comes up.
    Vibrance {
        amount: f32,
    },
    /// Monochrome, mixed by hand. Every colour becomes the weighted sum
    /// of its channels, and the weights are the whole point: a picture
    /// taken to grey by one recipe is a different picture from the same
    /// one taken to grey by another — a high red weight darkens a blue
    /// sky the way a red filter on the lens did.
    ///
    /// The weights are normalized by their own total, so moving one
    /// changes the mix rather than the brightness; exposure and levels
    /// are what brightness is for. [`LUMA`] is the default and is the
    /// answer for "just make it grey".
    BlackAndWhite {
        red: f32,
        green: f32,
        blue: f32,
    },
    /// Every tone replaced by the colour at its own place along a ramp:
    /// the shadows take the first stop, the highlights the last, and
    /// everything between is read off the ramp. Duotones, split tones
    /// and the whole family of graded looks are this one adjustment.
    ///
    /// Where a tone sits along the ramp is its brightness as a device
    /// shows it, not as light measures it — the same reason a tone curve
    /// is drawn over the display encoding, and what makes the middle of
    /// the ramp land on the tones that look middling.
    GradientMap {
        stops: Vec<GradientStop>,
    },
    /// Turned inside out: what was light is dark and every colour becomes
    /// its opposite. `amount` is how far to take it, so half way is the
    /// flat grey the two sides meet at.
    ///
    /// On the values a device shows rather than on light itself. Light
    /// inverted is not what anyone means by a negative: linear 0.5 shows
    /// as 188, so inverting in light would turn a middle grey into a
    /// near-black rather than into itself.
    Invert {
        amount: f32,
    },
    /// The two ends of the tone range, moved without touching the middle:
    /// `shadows` above zero opens up what is dark, `highlights` above
    /// zero pulls back what is bright, and both run to -1 for the
    /// opposite — deepening the shadows, brightening the highlights. It
    /// is the first thing asked of a photograph after exposure: a face
    /// against a window is dark because the window is bright, and no
    /// single exposure fixes both.
    ///
    /// Which end a pixel belongs to is decided by its brightness as a
    /// device shows it, and each end's pull falls off as the square of
    /// the distance from it, so the middle of the range is left alone.
    /// What moves is the pixel's brightness; its colour comes along
    /// unchanged, because a shadow lifted is the same colour it was.
    ///
    /// This is a function of the pixel and nothing else. Photoshop's
    /// version reads the neighbourhood too, which is what gives it local
    /// contrast and its halos; a per-pixel one cannot make a halo, and
    /// cannot tell a dark thing in bright surroundings from a dark thing
    /// in dark ones.
    ShadowsHighlights {
        shadows: f32,
        highlights: f32,
    },
}

/// How much each channel contributes to brightness — the Rec. 709
/// weights, which is what "luminance" means everywhere else in this
/// engine and the default recipe for [`Adjustment::BlackAndWhite`].
pub const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];

/// Non-destructive convolution filters; like adjustments they apply at
/// render time to everything composited below the layer. Unlike adjustments
/// they read pixel neighborhoods, so their invalidation is whole-canvas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Filter {
    /// `sigma` is the Gaussian standard deviation in document pixels.
    GaussianBlur { sigma: f32 },
    /// Unsharp mask: original + amount × (original − blur(sigma)).
    Sharpen { sigma: f32, amount: f32 },
    /// Squares of one colour each, the average of what they covered:
    /// what a face or a number is taken out of a picture with. `size` is
    /// the square's side in document pixels, and the grid is anchored in
    /// the document, so a block stays the same block when the page is
    /// panned or zoomed rather than crawling under the view.
    Pixelate { size: f32 },
    /// Grain. `amount` is how far a pixel can be moved, 0..=1; `grain`
    /// is how many document pixels across one speck is, so grain gets
    /// bigger with the picture rather than staying the size of a screen
    /// pixel; `mono` moves every channel together, which is film grain,
    /// against moving each on its own, which is sensor noise.
    ///
    /// It is a function of where a speck is in the document and of
    /// `seed`, not of any state carried between pixels, so the same page
    /// grains the same way every time it is drawn — which is what lets a
    /// filter be a live layer rather than something baked once.
    Noise {
        amount: f32,
        grain: f32,
        mono: bool,
        seed: u32,
    },
    /// The corners of the page taken down (or brought up), which is what
    /// a lens does and what a photographer does on purpose afterwards to
    /// hold the eye in the middle of a picture.
    ///
    /// `amount` above zero darkens and below zero lightens; `radius` is
    /// how far out from the middle nothing happens at all, as a fraction
    /// of the way to the corner; `softness` is how gradually it comes in
    /// once it does — 0 a straight ramp, 1 an eased one that has no edge
    /// anywhere.
    ///
    /// Measured from the middle of the page in document units, so it
    /// sits on the picture rather than on the window: panning slides the
    /// picture under it and the darkening stays where it is on the page.
    Vignette {
        amount: f32,
        radius: f32,
        softness: f32,
    },
}

/// A straight line the user placed to lay work out against. Not artwork —
/// guides never render and never export — but document state all the same,
/// because they belong to the layout rather than to the window it is being
/// viewed through.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Guide {
    /// A vertical line at this document x.
    Vertical(f32),
    /// A horizontal line at this document y.
    Horizontal(f32),
}

impl Guide {
    pub fn at(&self) -> f32 {
        match self {
            Guide::Vertical(v) | Guide::Horizontal(v) => *v,
        }
    }

    pub fn is_vertical(&self) -> bool {
        matches!(self, Guide::Vertical(_))
    }
}

/// Where each line sits inside the block's own width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// A live text object: the string and styling are the document state,
/// glyphs rasterize at render time (nothing is ever baked).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextSpec {
    pub text: String,
    /// Font size in document pixels (ascent-to-descent scale).
    pub size: f32,
    pub fill: AuthoredColor,
    /// Additive, all three: a document written before they existed loads
    /// with the values it was rendered at, so it still looks like itself.
    #[serde(default)]
    pub align: TextAlign,
    /// Line spacing as a multiple of the font's own line height.
    #[serde(default = "one")]
    pub line_height: f32,
    /// Tracking: extra space after each glyph, in ems, the way it is
    /// normally quoted — so it scales with the size rather than fighting it.
    #[serde(default)]
    pub letter_spacing: f32,
    /// Wrap width in document pixels. Zero is a block that fits its own
    /// text; anything else wraps words to that width and is that wide, so
    /// alignment inside it holds still while the text changes. A word
    /// longer than the width stands on its own line and overhangs.
    #[serde(default)]
    pub width: f32,
    /// Which face sets the block, by the name it was registered under.
    /// Empty — and any name nothing answers to — is the bundled face, so
    /// a document set in a font this machine lacks still renders.
    #[serde(default)]
    pub font: String,
    /// Italic: set in the face's "… Oblique" (or "… Italic") twin when one
    /// is registered, and leaned by the rasterizer when none is.
    #[serde(default)]
    pub italic: bool,
    /// Bold: set in the face's "… Bold" twin when one is registered, and
    /// thickened by the rasterizer when none is. Additive.
    #[serde(default)]
    pub bold: bool,
    /// A line under each line of text, and one through it, drawn by the
    /// rasterizer in the block's colour at sizes relative to the em.
    #[serde(default)]
    pub underline: bool,
    #[serde(default)]
    pub strike: bool,
    /// A guide to set the text along instead of in lines: a shape in the
    /// block's own space (copied from a layer when attached, so the block
    /// stands alone), with each glyph turned to follow it. Text that runs
    /// off an open guide's end is not drawn; a closed one wraps.
    #[serde(default)]
    pub along: Option<VectorShape>,
    /// How far along the guide, in document pixels, the text starts.
    #[serde(default)]
    pub along_offset: f32,
    /// Stretches of the text set differently from the rest of it.
    /// Additive: a block written before runs existed has none, and is
    /// set the way it always was.
    #[serde(default)]
    pub runs: Vec<StyleRun>,
}

/// A stretch of a block set differently from the rest: the same choices
/// the block itself makes, made again over a range of its text, with
/// `None` meaning "however the block does it".
///
/// A run's size is the block's — one line of text is one line tall here,
/// and mixing sizes inside a block is a different feature — so a run
/// changes how its letters are drawn, never where the lines sit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StyleRun {
    /// Byte offsets into the block's text. Ranges are clipped to the
    /// text and to each other when the block is set, so text can be
    /// deleted without the runs having to be repaired first.
    pub start: usize,
    pub end: usize,
    #[serde(default)]
    pub fill: Option<AuthoredColor>,
    #[serde(default)]
    pub bold: Option<bool>,
    #[serde(default)]
    pub italic: Option<bool>,
    #[serde(default)]
    pub underline: Option<bool>,
    #[serde(default)]
    pub strike: Option<bool>,
    #[serde(default)]
    pub font: Option<String>,
}

impl StyleRun {
    /// A run over `start..end` that changes nothing yet.
    pub fn over(start: usize, end: usize) -> Self {
        Self {
            start,
            end,
            fill: None,
            bold: None,
            italic: None,
            underline: None,
            strike: None,
            font: None,
        }
    }

    /// Whether the run asks for anything at all. One that does not is
    /// dropped rather than kept as a range that changes nothing.
    pub fn says_anything(&self) -> bool {
        self.fill.is_some()
            || self.bold.is_some()
            || self.italic.is_some()
            || self.underline.is_some()
            || self.strike.is_some()
            || self.font.is_some()
    }
}

fn one() -> f32 {
    1.0
}

impl TextSpec {
    /// A block with default styling; the three additive fields have to come
    /// from somewhere when the spec is built in Rust rather than parsed.
    pub fn new(text: impl Into<String>, size: f32, fill: AuthoredColor) -> Self {
        Self {
            text: text.into(),
            size,
            fill,
            align: TextAlign::default(),
            line_height: 1.0,
            letter_spacing: 0.0,
            width: 0.0,
            font: String::new(),
            italic: false,
            bold: false,
            underline: false,
            strike: false,
            along: None,
            along_offset: 0.0,
            runs: Vec::new(),
        }
    }

    /// The line height actually used, guarded against a zero or negative
    /// multiple that would collapse every line onto one.
    pub fn line_scale(&self) -> f32 {
        self.line_height.max(0.05)
    }

    /// The block's text as a sequence of pieces, each with the index of
    /// the run that governs it or `None` where the block's own styling
    /// stands. Pieces are in order, cover the whole text exactly once,
    /// and always begin and end on a character boundary.
    ///
    /// This is the one place that decides which run a byte belongs to, so
    /// every part of the pipeline — measuring, shaping, rasterizing and
    /// both exporters — cuts the text the same way. Ranges are sorted,
    /// clipped to the text, and trimmed against whatever came before, so
    /// runs left overlapping or hanging past the end by an edit are read
    /// sensibly rather than refused.
    pub fn pieces(&self) -> Vec<(usize, usize, Option<usize>)> {
        let len = self.text.len();
        let mut order: Vec<usize> = (0..self.runs.len()).collect();
        order.sort_by_key(|&i| (self.runs[i].start, self.runs[i].end));
        let mut out: Vec<(usize, usize, Option<usize>)> = Vec::new();
        let mut at = 0usize;
        for i in order {
            let run = &self.runs[i];
            let start = self.boundary(run.start.max(at).min(len));
            let end = self.boundary(run.end.min(len)).max(start);
            if start >= end {
                continue;
            }
            if start > at {
                out.push((at, start, None));
            }
            out.push((start, end, Some(i)));
            at = end;
        }
        if at < len || out.is_empty() {
            out.push((at, len, None));
        }
        out
    }

    /// The nearest character boundary at or before `at`: a run whose end
    /// an edit left in the middle of a character still cuts the text
    /// somewhere it can be cut.
    fn boundary(&self, at: usize) -> usize {
        let mut at = at.min(self.text.len());
        while at > 0 && !self.text.is_char_boundary(at) {
            at -= 1;
        }
        at
    }

    /// This block's styling with `run`'s choices laid over it: how one
    /// piece of the text is actually set.
    ///
    /// The text itself is left empty — the piece it applies to is named
    /// separately, and copying the whole block's text once per piece is
    /// the sort of thing that makes wrapping a long block slow.
    pub fn styling_under(&self, run: Option<&StyleRun>) -> TextSpec {
        let mut spec = self.clone();
        spec.text = String::new();
        spec.runs = Vec::new();
        let Some(run) = run else {
            return spec;
        };
        if let Some(fill) = run.fill {
            spec.fill = fill;
        }
        if let Some(bold) = run.bold {
            spec.bold = bold;
        }
        if let Some(italic) = run.italic {
            spec.italic = italic;
        }
        if let Some(underline) = run.underline {
            spec.underline = underline;
        }
        if let Some(strike) = run.strike {
            spec.strike = strike;
        }
        if let Some(font) = &run.font {
            spec.font = font.clone();
        }
        spec
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NodeKind {
    Group,
    /// A layer painted with a brush: the strokes that were laid on it,
    /// in the order they were laid, still live — each one can be taken
    /// off again, and the layer is re-rendered rather than kept as
    /// pixels.
    Paint {
        strokes: Vec<PaintStroke>,
    },
    /// A layer that paints with what is already under it. Each stroke
    /// carries the offset it reads at, so the picture it lays down is
    /// whatever the page shows there *now* — retouch the source and the
    /// clone follows, which is what makes this non-destructive where a
    /// stamped copy of pixels would not be.
    Clone {
        strokes: Vec<PaintStroke>,
    },
    Vector {
        shape: VectorShape,
        fill: Option<AuthoredColor>,
        stroke: Option<Stroke>,
        /// Paints in place of `fill` when set. Additive, so documents
        /// written before gradients existed still load.
        #[serde(default)]
        gradient: Option<Gradient>,
    },
    Raster(RasterRef),
    Adjustment(Adjustment),
    Filter(Filter),
    Text(TextSpec),
    /// A live copy of another layer: it draws whatever that layer holds,
    /// wherever this one is put, so changing the original changes every
    /// copy of it. Its own transform, opacity, blend and mask are its
    /// own; only what it is a picture *of* is shared.
    ///
    /// The original's own transform is not part of what travels — an
    /// instance places the picture itself — so moving the original moves
    /// only the original.
    Instance {
        of: NodeId,
        /// Which of the original's layers this copy stands in for with
        /// one of its own: `replaces[k]` is the position, among the
        /// original's children, that this copy's `k`th child takes.
        ///
        /// That is what makes a copy differ where it has to — a label
        /// with a different string, a panel in a different colour — while
        /// everything else still follows the original. A child that
        /// stands in for nothing is drawn after the original's contents,
        /// so dropping a layer into a copy adds to it rather than
        /// breaking it.
        #[serde(default)]
        replaces: Vec<usize>,
    },
    /// A frame on the page: a group with a size of its own that cuts its
    /// contents to that box, paints a ground behind them, and exports at
    /// exactly that many pixels however it is placed or scaled. What lets
    /// one document hold several pictures — three screen sizes, a set of
    /// cards, a poster and its detail — instead of one.
    Artboard {
        width: f32,
        height: f32,
        /// Painted behind the contents, inside the frame. `None` is a
        /// frame that shows what is under it on the page.
        background: Option<AuthoredColor>,
        /// How many pixels a pixel of the frame exports as. A frame is
        /// usually made at the size it is designed at and wanted at
        /// twice or three times that, and which multiple belongs to
        /// which frame is a property of the frame rather than a thing to
        /// remember at the moment of exporting. Additive: a file written
        /// before this reads as 1, and so does anything at or below
        /// zero, since a frame exporting as nothing is not a wish anyone
        /// has.
        #[serde(default = "one")]
        export_scale: f32,
    },
}

impl NodeKind {
    /// Whether the node is one that holds other layers — what a parent
    /// has to be for anything to go into it.
    pub fn holds_children(&self) -> bool {
        matches!(
            self,
            // A copy holds the layers it stands in for.
            NodeKind::Group | NodeKind::Artboard { .. } | NodeKind::Instance { .. }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub color: AuthoredColor,
    pub width: f32,
    /// Per-anchor width multipliers in 0..=1, scaling `width` along the
    /// path so a stroke can swell and taper. Empty means a constant width,
    /// which is why older files keep loading; a path's own anchors index
    /// it, and the renderer interpolates between them.
    #[serde(default)]
    pub widths: Vec<f32>,
    /// A dash pattern along the outline: lengths in the shape's own
    /// units, on and off in turn and repeating, so `[6.0, 3.0]` is six
    /// on and three off. One length alone is that much on and the same
    /// off. Empty is a solid stroke, which is why older files still
    /// load. A dashed stroke is drawn at one width — a stroke that
    /// swells and one that is broken up are two different ideas about
    /// the same line, and the dashes win.
    #[serde(default)]
    pub dash: Vec<f32>,
    /// How the stroke ends where the line stops — including at either
    /// end of every dash. Paths only: a rect's or an ellipse's stroke is
    /// a band lying inside a closed outline, which never stops.
    #[serde(default)]
    pub cap: StrokeCap,
    /// How the stroke turns a corner. Paths only, for the same reason.
    #[serde(default)]
    pub join: StrokeJoin,
    /// Which side of the outline the stroke lies on. `None` is whatever
    /// this shape has always been stroked as — see [`stroke_align`] —
    /// which is what keeps a file written before there was a choice
    /// drawing the way it did.
    #[serde(default)]
    pub align: Option<StrokeAlign>,
    /// What sits at the line's first point, and at its last. A line has
    /// two ends and they are asked separately, since an arrow at one end
    /// and nothing at the other is what most arrows are.
    ///
    /// An open path only: a closed ring, a rect and an ellipse have no
    /// ends to put anything on. These go where the *line* stops, not
    /// where each dash does, so a dashed arrow has one head and not a
    /// dozen.
    #[serde(default)]
    pub start_marker: Marker,
    #[serde(default)]
    pub end_marker: Marker,
}

/// What a line carries at an end.
///
/// Sized from the stroke's own width rather than given a size of its
/// own, which is what SVG's `markerUnits="strokeWidth"` means and what
/// keeps a head in proportion to the line it is on when the line is made
/// thicker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Marker {
    #[default]
    None,
    /// A head pointing away from the line, its tip on the last point.
    Arrow,
    /// A tick across the line, as a dimension line is finished.
    Bar,
    /// A disc centred on the last point.
    Dot,
}

/// How far an arrowhead reaches back from its tip, and how far a head or
/// a bar reaches to either side, both as multiples of the stroke's width.
pub const MARKER_LENGTH: f32 = 3.0;
pub const MARKER_REACH: f32 = 1.5;

/// Which side of the outline a stroke lies on.
///
/// Asked of a rect and an ellipse, whose outlines have a distance of
/// their own. A path is stroked down the middle of its line whatever it
/// asks for — see [`stroke_align`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StrokeAlign {
    /// The band lies inside the outline, so thickening a border never
    /// grows the shape.
    Inside,
    /// Half in and half out, which is what SVG and PDF do by default.
    Centre,
    /// The band lies outside the outline, so a border never eats into
    /// the fill.
    Outside,
}

/// Which side of the outline this stroke actually lies on.
///
/// Where nothing was asked for, the answer is what the engine did before
/// there could be an ask: a rect's and an ellipse's stroke was a band
/// inside the edge, so thickening a border never grew the shape, and a
/// path's ran down the middle of its line. That is an inconsistency —
/// the same shape drawn two ways stroked two ways — which is the reason
/// for the ask, but a file written then still has to draw as it did.
pub fn stroke_align(shape: &VectorShape, stroke: &Stroke) -> StrokeAlign {
    match shape {
        // A rect's and an ellipse's outline has a distance of its own,
        // so a band to either side of it is exact — a band inside, one
        // straddling, one outside, and the dashes that break any of them
        // up walk the middle of the band they belong to.
        VectorShape::Rect { .. } | VectorShape::Ellipse { .. } => {
            stroke.align.unwrap_or(StrokeAlign::Inside)
        }
        // A path is stroked down the middle of its line, which is what a
        // line means and what an open one can only mean. Putting a band
        // to one side of a closed path asks for its outline offset, and
        // an offset outline is a guess where a distance is not.
        VectorShape::Path { .. } => StrokeAlign::Centre,
    }
}

/// How a stroke ends where its line stops.
///
/// Round is the default because it is what the engine drew before there
/// was a choice — a stroke was a distance from the line, and a distance
/// rounds every end — so a file written then still reads the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum StrokeCap {
    /// Flat on the last point: the line stops exactly where it ends,
    /// which is what a rule, a tick and a dash want.
    Butt,
    #[default]
    Round,
    /// Flat half a width past the last point, so the line ends square.
    Square,
}

/// How a stroke turns the corner at a point where two segments meet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum StrokeJoin {
    /// Carried out to the point where the two outer edges cross, which
    /// is what makes a drawn corner look drawn — falling back to a bevel
    /// where that point would run away, past [`MITER_LIMIT`] widths.
    Miter,
    #[default]
    Round,
    /// Cut straight across the corner.
    Bevel,
}

/// How far a miter may reach, as a multiple of the stroke's half-width,
/// before the corner is cut off instead. Four is what SVG and PDF both
/// take as read, and it is the angle (about 29°) below which a miter
/// stops being a corner and becomes a spike.
pub const MITER_LIMIT: f32 = 4.0;

/// A non-destructive mask attachable to any node: it modulates the node's
/// output (a group's composite, a shape's paint, an adjustment's or filter's
/// strength) by per-pixel coverage. Mask geometry lives in document space,
/// independent of the node's own transform.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mask {
    pub kind: MaskKind,
    /// Flip coverage: masked-out becomes masked-in.
    pub invert: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MaskKind {
    /// Hard-edged coverage from a shape (1 inside, 0 outside).
    Vector {
        shape: VectorShape,
        transform: Transform,
    },
    /// Greyscale coverage from an image resource: luminance × alpha
    /// (white shows, black hides), sampled through the transform.
    Raster {
        resource_id: String,
        width: u32,
        height: u32,
        transform: Transform,
    },
    /// Coverage brushed on by hand. It starts showing everything, and
    /// each stroke either hides what it covers (an eraser) or shows it
    /// again (a brush) — which is how a piece is taken out of a layer
    /// without touching the layer.
    Painted { strokes: Vec<PaintStroke> },
}

/// How a layer answers when the frame around it changes size — the same
/// question on each axis, and the reason a frame can be given a new size
/// at all rather than only ever being drawn at the one it was made with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Pin {
    /// Keeps its distance from the left (or top) edge, which is what a
    /// layer does when nothing is said — and what every layer written
    /// before pinning existed did.
    #[default]
    Start,
    /// Keeps its distance from the right (or bottom) edge.
    End,
    /// Keeps its distance from the middle.
    Middle,
    /// Keeps both distances, so it grows and shrinks with the frame.
    Stretch,
}

/// A layer's answer on each axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Pinning {
    pub x: Pin,
    pub y: Pin,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub name: String,
    pub kind: NodeKind,
    pub transform: Transform,
    pub opacity: f32,
    pub visible: bool,
    pub blend: BlendMode,
    #[serde(default)]
    pub mask: Option<Mask>,
    /// Effects drawn around the layer, bottom-most first. Live parameters
    /// like everything else: the layer's own pixels are untouched.
    #[serde(default)]
    pub effects: Vec<Effect>,
    /// A locked layer cannot be picked or moved on the canvas; it still
    /// renders and can still be reached through the panel. Additive.
    #[serde(default)]
    pub locked: bool,
    /// Confined to the layer below it: the layer shows only where that one
    /// does, and inherits its fate — hide the layer below and this one goes
    /// with it. A run of them stacks against the same layer, which is the
    /// nearest unclipped one underneath. The bottom layer of a parent has
    /// nothing to clip to, so the flag is ignored there. Additive.
    #[serde(default)]
    pub clipped: bool,
    /// What the layer does when the frame holding it is given a new
    /// size. Read only inside a frame; additive, and its default is what
    /// every layer did before it existed.
    #[serde(default)]
    pub pinned: Pinning,
}

/// A live effect attached to a layer. Effects are rendered from the layer's
/// own composite, so they follow it when it moves, turns, or changes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Effect {
    /// The layer's silhouette, blurred and offset, painted behind it.
    /// `dx`/`dy` and `blur` are in the layer's parent space, so a shadow
    /// scales and turns with the group it is in.
    DropShadow {
        dx: f32,
        dy: f32,
        blur: f32,
        color: AuthoredColor,
        opacity: f32,
    },
    /// A band of colour hugging the layer's silhouette from outside, the
    /// way a headline is given an outline. `width` is in the layer's
    /// parent space.
    Outline {
        width: f32,
        color: AuthoredColor,
        opacity: f32,
    },
    /// The same shadow, cast inward: it darkens the inside of the
    /// silhouette against its own edge and never leaves it.
    InnerShadow {
        dx: f32,
        dy: f32,
        blur: f32,
        color: AuthoredColor,
        opacity: f32,
    },
}

impl Effect {
    /// How far, in the space the effect is written in, it can reach beyond
    /// the layer's own bounds — what bounds and dirty regions must grow by.
    pub fn reach(&self) -> f32 {
        match self {
            // Three iterated box blurs reach about 3σ; round up generously,
            // then add the offset, which can point either way.
            Effect::DropShadow { dx, dy, blur, .. } => {
                blur.abs() * 3.0 + dx.abs().max(dy.abs()) + 2.0
            }
            Effect::Outline { width, .. } => width.abs() + 2.0,
            // An inner shadow stays inside the layer, but it still needs
            // the silhouette read from a ring outside it to know where the
            // edge is.
            Effect::InnerShadow { dx, dy, blur, .. } => {
                blur.abs() * 3.0 + dx.abs().max(dy.abs()) + 2.0
            }
        }
    }

    /// Whether the effect is painted over the layer rather than behind it.
    /// An inner shadow is inside the silhouette, so it has to land on top
    /// of the pixels it is shading.
    pub fn over(&self) -> bool {
        matches!(self, Effect::InnerShadow { .. })
    }

    /// A short name for history labels and the effect list.
    pub fn label(&self) -> &'static str {
        match self {
            Effect::DropShadow { .. } => "Drop shadow",
            Effect::Outline { .. } => "Outline",
            Effect::InnerShadow { .. } => "Inner shadow",
        }
    }
}

impl Node {
    fn base(name: &str, kind: NodeKind) -> Self {
        Self {
            name: name.to_string(),
            kind,
            transform: Transform::default(),
            opacity: 1.0,
            visible: true,
            blend: BlendMode::Normal,
            mask: None,
            effects: Vec::new(),
            locked: false,
            clipped: false,
            pinned: Pinning::default(),
        }
    }

    pub fn group(name: &str) -> Self {
        Self::base(name, NodeKind::Group)
    }

    /// A live copy of `of` — see [`NodeKind::Instance`].
    pub fn instance(name: &str, of: NodeId) -> Self {
        Self::base(
            name,
            NodeKind::Instance {
                of,
                replaces: Vec::new(),
            },
        )
    }

    pub fn artboard(
        name: &str,
        width: f32,
        height: f32,
        background: Option<AuthoredColor>,
    ) -> Self {
        Self::base(
            name,
            NodeKind::Artboard {
                width,
                height,
                background,
                export_scale: 1.0,
            },
        )
    }

    pub fn vector(name: &str, shape: VectorShape) -> Self {
        Self::base(
            name,
            NodeKind::Vector {
                shape,
                fill: None,
                stroke: None,
                gradient: None,
            },
        )
    }

    pub fn raster(name: &str, raster: RasterRef) -> Self {
        Self::base(name, NodeKind::Raster(raster))
    }

    pub fn filter(name: &str, filter: Filter) -> Self {
        Self::base(name, NodeKind::Filter(filter))
    }

    /// An empty layer to clone onto.
    pub fn clone_layer(name: &str) -> Self {
        Self::base(
            name,
            NodeKind::Clone {
                strokes: Vec::new(),
            },
        )
    }

    /// An empty layer to paint on.
    pub fn paint(name: &str) -> Self {
        Self::base(
            name,
            NodeKind::Paint {
                strokes: Vec::new(),
            },
        )
    }

    pub fn text(name: &str, spec: TextSpec) -> Self {
        Self::base(name, NodeKind::Text(spec))
    }

    pub fn adjustment(name: &str, adjustment: Adjustment) -> Self {
        Self::base(name, NodeKind::Adjustment(adjustment))
    }
}

#[cfg(test)]
mod tests {

    /// A stroke written before there was a choice says nothing about how
    /// it ends or turns, and has to come back the way it was drawn then:
    /// a distance from the line, which rounds every end and every corner.
    #[test]
    fn a_stroke_from_before_the_choice_reads_as_round() {
        let old = r#"{"color":{"Srgb":{"r":1,"g":0,"b":0,"a":1}},"width":4}"#;
        let stroke: Stroke = serde_json::from_str(old).unwrap();
        assert_eq!(
            (stroke.cap, stroke.join),
            (StrokeCap::Round, StrokeJoin::Round)
        );
        assert!(stroke.dash.is_empty() && stroke.widths.is_empty());
    }
    use super::*;

    fn block(text: &str, runs: Vec<StyleRun>) -> TextSpec {
        let mut spec = TextSpec::new(
            text,
            12.0,
            AuthoredColor::Srgb {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
        );
        spec.runs = runs;
        spec
    }

    /// The pieces cover the text once, in order, whatever the runs do:
    /// none at all, one in the middle, two that overlap, one hanging
    /// past the end, and one that starts where the last one stopped.
    #[test]
    fn the_pieces_cover_the_text_once_however_the_runs_lie() {
        let cover = |spec: &TextSpec| {
            let pieces = spec.pieces();
            let mut at = 0;
            for (start, end, _) in &pieces {
                assert_eq!(*start, at, "pieces run on from each other: {pieces:?}");
                assert!(end >= start, "and never backwards: {pieces:?}");
                at = *end;
            }
            assert_eq!(at, spec.text.len(), "to the end: {pieces:?}");
            pieces
        };

        assert_eq!(cover(&block("hello", vec![])), [(0, 5, None)]);
        assert_eq!(
            cover(&block("hello", vec![StyleRun::over(1, 3)])),
            [(0, 1, None), (1, 3, Some(0)), (3, 5, None)]
        );
        // Overlapping: the one that starts first keeps what it has and
        // the other picks up where that left off.
        assert_eq!(
            cover(&block(
                "hello",
                vec![StyleRun::over(0, 3), StyleRun::over(2, 5)]
            )),
            [(0, 3, Some(0)), (3, 5, Some(1))]
        );
        // Hanging past the end, which an edit that shortened the text
        // would leave behind.
        assert_eq!(
            cover(&block("hi", vec![StyleRun::over(1, 900)])),
            [(0, 1, None), (1, 2, Some(0))]
        );
        // Given out of order, and abutting.
        assert_eq!(
            cover(&block(
                "abcd",
                vec![StyleRun::over(2, 4), StyleRun::over(0, 2)]
            )),
            [(0, 2, Some(1)), (2, 4, Some(0))]
        );
        // Empty text still has one piece to hand back.
        assert_eq!(
            cover(&block("", vec![StyleRun::over(0, 4)])),
            [(0, 0, None)]
        );
    }

    /// A range that an edit left in the middle of a character is cut
    /// where the character starts, not through it.
    #[test]
    fn a_piece_never_cuts_a_character_in_half() {
        // "é" is two bytes, "😀" is four.
        let spec = block("aé😀b", vec![StyleRun::over(2, 6)]);
        for (start, end, _) in spec.pieces() {
            assert!(
                spec.text.is_char_boundary(start) && spec.text.is_char_boundary(end),
                "{start}..{end} of {:?}",
                spec.text
            );
        }
        let mut inside = block("aé😀b", vec![StyleRun::over(2, 5)]);
        inside.runs[0].start = 2;
        let cut: Vec<_> = inside.pieces().iter().map(|p| (p.0, p.1)).collect();
        assert_eq!(cut, [(0, 1), (1, 3), (3, 8)], "clipped back to a boundary");
    }

    /// A run says only what it changes; everything else is the block's.
    #[test]
    fn a_run_lays_its_choices_over_the_block_and_no_more() {
        let mut spec = block("hi", vec![]);
        spec.italic = true;
        spec.font = "Some Face".into();
        let mut run = StyleRun::over(0, 1);
        run.bold = Some(true);
        run.italic = Some(false);
        let under = spec.styling_under(Some(&run));
        assert!(under.bold && !under.italic, "what the run asked for");
        assert_eq!(under.font, "Some Face", "and the block's for the rest");
        assert_eq!(under.size, spec.size);
        assert!(under.runs.is_empty(), "the piece is not styled again");
        assert!(
            spec.styling_under(None).italic,
            "no run is the block itself"
        );
    }
}
