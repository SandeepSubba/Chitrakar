// One quad per shape, in document space; the fragment finds its coverage
// from the shape's own signed distance, so an edge is as smooth as the
// pixel it lands on rather than as coarse as the mesh.

struct VsOut {
    @builtin(position) pos: vec4f,
    @location(0) local: vec2f,
    @location(1) @interpolate(flat) params: vec4f,
    @location(2) @interpolate(flat) color: vec4f,
    @location(3) @interpolate(flat) grad: vec4f,
};

struct Page {
    size: vec2f,
    pad: vec2f,
};

@group(0) @binding(0) var<uniform> page: Page;

// Document pixels to clip space, y downwards as the document has it.
fn clip(doc: vec2f) -> vec4f {
    return vec4f(doc.x / page.size.x * 2.0 - 1.0, 1.0 - doc.y / page.size.y * 2.0, 0.0, 1.0);
}

@vertex
fn vs(
    @location(0) doc: vec2f,
    @location(1) local: vec2f,
    @location(2) params: vec4f,
    @location(3) color: vec4f,
    @location(4) grad: vec4f,
) -> VsOut {
    var out: VsOut;
    out.pos = clip(doc);
    out.local = local;
    out.params = params;
    out.color = color;
    out.grad = grad;
    return out;
}

// Signed distance to a rounded rectangle whose top-left is the origin.
fn rect_distance(p: vec2f, size: vec2f, r: f32) -> f32 {
    let half = size * 0.5;
    let q = abs(p - half) - (half - vec2f(r, r));
    return length(max(q, vec2f(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - r;
}

// An ellipse's implicit function about `c` with radii `r`, divided by
// its own gradient: the first-order distance to the rim, which is what
// an edge a pixel wide needs.
fn ellipse_distance(p: vec2f, c: vec2f, r: vec2f) -> f32 {
    let k = (p - c) / r;
    let f = dot(k, k) - 1.0;
    let g = 2.0 * vec2f(k.x / r.x, k.y / r.y);
    return f / max(length(g), 1e-6);
}

// How much of this pixel a signed distance covers. The soft band is one
// device pixel wide however the shape is transformed: fwidth measures
// the distance's own rate of change on the screen. Derivatives have to
// be taken in uniform control flow, which is why every distance below
// is computed and only then selected between.
fn edge(d: f32) -> f32 {
    return clamp(0.5 - d / max(fwidth(d), 1e-6), 0.0, 1.0);
}

// How much of this pixel the shape covers. `params.w` says which shape
// it is — 0 a rounded rectangle, 1 an ellipse, and 2 or 3 the same two
// as a stroke: the innermost `grad.x` of the shape, which is where the
// CPU renderer puts a rect's or an ellipse's stroke so that stroking
// one never grows its bounds.
fn coverage(in: VsOut) -> f32 {
    let size = in.params.xy;
    let r = size * 0.5;
    let band = in.params.w >= 2.0;
    let ellipse = in.params.w - select(0.0, 2.0, band) > 0.5;
    let width = in.grad.x;

    let outer = select(
        rect_distance(in.local, size, in.params.z),
        ellipse_distance(in.local, r, r),
        ellipse,
    );
    // The inside edge of a band. A rounded rect's is its own distance
    // pushed in by the width; an ellipse's is the ellipse shrunk by the
    // width on each axis, which is a different curve — and once that has
    // shrunk to nothing the band is the whole inside.
    let shrunk = r - vec2f(width, width);
    let inner = select(
        outer + width,
        select(ellipse_distance(in.local, r, max(shrunk, vec2f(1e-6, 1e-6))), 1e9, shrunk.x <= 0.0 || shrunk.y <= 0.0),
        ellipse,
    );
    let cov = edge(outer);
    return select(cov, clamp(cov - edge(inner), 0.0, 1.0), band);
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4f {
    return in.color * coverage(in);
}

// Stencil pass for a path: nothing but position, no colour written. The
// triangles of a fan over its rings flip the stencil, so a pixel ends up
// set exactly where an even-odd fill covers it.
@vertex
fn vs_stencil(@location(0) doc: vec2f) -> @builtin(position) vec4f {
    return clip(doc);
}

// A pipeline in a pass that has a colour attachment must name one too,
// even when — as here — it writes nothing to it.
@fragment
fn fs_stencil() -> @location(0) vec4f {
    return vec4f(0.0, 0.0, 0.0, 0.0);
}

// The cover pass paints the path's colour wherever the stencil says the
// fill reached, and clears the stencil behind it.
struct CoverOut {
    @builtin(position) pos: vec4f,
    @location(0) uv: vec2f,
    @location(1) @interpolate(flat) color: vec4f,
    @location(2) @interpolate(flat) grad: vec4f,
};

@vertex
fn vs_cover(
    @location(0) doc: vec2f,
    @location(1) local: vec2f,
    @location(3) color: vec4f,
    @location(4) grad: vec4f,
) -> CoverOut {
    var out: CoverOut;
    out.pos = clip(doc);
    out.uv = local;
    out.color = color;
    out.grad = grad;
    return out;
}

@fragment
fn fs_cover(in: CoverOut) -> @location(0) vec4f {
    return in.color;
}

// A placed image: the quad's own coordinates are its texture coordinates,
// and the texels are already premultiplied linear, so the filtering
// happens in the same space the compositor works in.
//
// The same binding carries a gradient's ramp — a single row of texels,
// its stops resolved and premultiplied on the CPU — so the two share a
// bind group layout and a sampler.
struct ImageOut {
    @builtin(position) pos: vec4f,
    @location(0) uv: vec2f,
    @location(1) @interpolate(flat) alpha: f32,
};

@group(1) @binding(0) var image: texture_2d<f32>;
@group(1) @binding(1) var image_sampler: sampler;

@vertex
fn vs_image(@location(0) doc: vec2f, @location(1) uv: vec2f, @location(3) color: vec4f) -> ImageOut {
    var out: ImageOut;
    out.pos = clip(doc);
    out.uv = uv;
    out.alpha = color.a;
    return out;
}

@fragment
fn fs_image(in: ImageOut) -> @location(0) vec4f {
    return textureSample(image, image_sampler, in.uv) * in.alpha;
}

// A text layer: the whole block rasterized to coverage at the size it
// is seen at, which is what the CPU renderer samples too, so the two
// read the same bitmap the same way. The coordinates are the raster's
// own texels; the row and column of transparent padding around it are
// what let the sampler fade off the edge instead of smearing it, and
// left of or above the block's origin there is no ink at all.
@fragment
fn fs_text(in: CoverOut) -> @location(0) vec4f {
    let size = vec2f(textureDimensions(image));
    let cov = textureSampleLevel(image, image_sampler, (in.uv + vec2f(1.0, 1.0)) / size, 0.0).r;
    let inked = in.uv.x >= 0.0 && in.uv.y >= 0.0;
    return in.color * select(0.0, cov, inked);
}

// Where a point of the shape's normalized box sits along its gradient:
// the projection onto the line from `from` to `to`, or the distance from
// the centre in units of the radius, clamped past either end — the same
// arithmetic the CPU renderer does per pixel.
fn ramp_at(uv: vec2f, geom: vec4f, radial: bool) -> f32 {
    if radial {
        if geom.z < 1e-6 {
            return 1.0;
        }
        return clamp(length(uv - geom.xy) / geom.z, 0.0, 1.0);
    }
    let d = geom.zw - geom.xy;
    let len2 = dot(d, d);
    if len2 < 1e-12 {
        return 0.0;
    }
    return clamp(dot(uv - geom.xy, d) / len2, 0.0, 1.0);
}

// The ramp's colour at `t`. The row's first and last texels are the ends
// of the ramp, so t maps onto their centres and the sampler interpolates
// the rest.
fn ramp_color(t: f32) -> vec4f {
    let n = f32(textureDimensions(image).x);
    let u = (t * (n - 1.0) + 0.5) / n;
    return textureSampleLevel(image, image_sampler, vec2f(u, 0.5), 0.0);
}

// A gradient-filled rectangle or ellipse: coverage as any other shape,
// colour from the ramp. `color` carries only which gradient this is (in
// r) and the layer's alpha (in a) — the paint itself is in the texture.
@fragment
fn fs_shape_gradient(in: VsOut) -> @location(0) vec4f {
    let cov = coverage(in);
    let uv = in.local / max(in.params.xy, vec2f(1e-6, 1e-6));
    return ramp_color(ramp_at(uv, in.grad, in.color.r > 0.5)) * in.color.a * cov;
}

// A gradient-filled path: the stencil already said where the fill
// reached, so the cover quad only has to say what colour it is. Its
// corners carry the normalized box coordinates, which interpolate
// across the quad however the layer is transformed.
@fragment
fn fs_cover_gradient(in: CoverOut) -> @location(0) vec4f {
    return ramp_color(ramp_at(in.uv, in.grad, in.color.r > 0.5)) * in.color.a;
}
