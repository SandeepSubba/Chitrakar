//! Node types of the scene graph. Every node is a live object described by
//! parameters — nothing here ever stores baked pixels except the immutable
//! source resource a [`RasterRef`] points at.

use chitrakar_color::AuthoredColor;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
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
}

/// Non-destructive convolution filters; like adjustments they apply at
/// render time to everything composited below the layer. Unlike adjustments
/// they read pixel neighborhoods, so their invalidation is whole-canvas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Filter {
    /// `sigma` is the Gaussian standard deviation in document pixels.
    GaussianBlur { sigma: f32 },
    /// Unsharp mask: original + amount × (original − blur(sigma)).
    Sharpen { sigma: f32, amount: f32 },
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
            underline: false,
            strike: false,
            along: None,
            along_offset: 0.0,
        }
    }

    /// The line height actually used, guarded against a zero or negative
    /// multiple that would collapse every line onto one.
    pub fn line_scale(&self) -> f32 {
        self.line_height.max(0.05)
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
    },
}

impl NodeKind {
    /// Whether the node is one that holds other layers — what a parent
    /// has to be for anything to go into it.
    pub fn holds_children(&self) -> bool {
        matches!(self, NodeKind::Group | NodeKind::Artboard { .. })
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
}

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
        }
    }

    pub fn group(name: &str) -> Self {
        Self::base(name, NodeKind::Group)
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
