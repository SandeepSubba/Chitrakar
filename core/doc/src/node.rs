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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum VectorShape {
    Rect {
        width: f32,
        height: f32,
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

/// A live text object: the string and styling are the document state,
/// glyphs rasterize at render time (nothing is ever baked).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextSpec {
    pub text: String,
    /// Font size in document pixels (ascent-to-descent scale).
    pub size: f32,
    pub fill: AuthoredColor,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NodeKind {
    Group,
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
        }
    }

    pub fn group(name: &str) -> Self {
        Self::base(name, NodeKind::Group)
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

    pub fn text(name: &str, spec: TextSpec) -> Self {
        Self::base(name, NodeKind::Text(spec))
    }

    pub fn adjustment(name: &str, adjustment: Adjustment) -> Self {
        Self::base(name, NodeKind::Adjustment(adjustment))
    }
}
