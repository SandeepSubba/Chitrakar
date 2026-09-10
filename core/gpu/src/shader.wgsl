// One quad per shape, in document space; the fragment finds its coverage
// from the shape's own signed distance, so an edge is as smooth as the
// pixel it lands on rather than as coarse as the mesh.

struct VsOut {
    @builtin(position) pos: vec4f,
    @location(0) local: vec2f,
    @location(1) @interpolate(flat) params: vec4f,
    @location(2) @interpolate(flat) color: vec4f,
    @location(3) @interpolate(flat) grad: vec4f,
    // Where this fragment is on the page, and the box the layer's mask
    // was rasterized over — the two together say where to read it.
    @location(4) page: vec2f,
    @location(5) @interpolate(flat) mask: vec4f,
};

struct Page {
    /// The surface being drawn, in device pixels.
    size: vec2f,
    /// Where the page lands on it: the first pixel inside, and the first
    /// past the end. The two are the whole surface while the backend
    /// draws the page at its own size, and part company under a view.
    /// What reads a neighbourhood stops at this rather than at the
    /// surface's edge — which is where the CPU renderer's own reading
    /// stops, since it is handed the page's rectangle to work in.
    lo: vec2f,
    hi: vec2f,
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
    @location(5) mask: vec4f,
) -> VsOut {
    var out: VsOut;
    out.pos = clip(doc);
    out.local = local;
    out.params = params;
    out.color = color;
    out.grad = grad;
    out.page = doc;
    out.mask = mask;
    return out;
}

// A layer's mask: the coverage it lets through, rasterized by the CPU
// renderer over a box of page pixels — the same reading the CPU
// compositor does at every pixel of a masked layer, so a mask cannot
// come to mean two things depending on which renderer drew it. A box of
// no width says the layer has no mask, which is most of them.
@group(2) @binding(0) var mask_tex: texture_2d<f32>;
@group(2) @binding(1) var mask_sampler: sampler;

fn mask_cover(page: vec2f, box: vec4f) -> f32 {
    if box.z <= 0.0 || box.w <= 0.0 {
        return 1.0;
    }
    return textureSampleLevel(mask_tex, mask_sampler, (page - box.xy) / box.zw, 0.0).r;
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
// as a stroke, whose band lies between two outlines: the shape grown by
// `grad.y` and the shape shrunk by `grad.x`. Which side of its own edge
// a band lies on is the difference between those two, so all three
// answers are the same arithmetic.
fn coverage(in: VsOut) -> f32 {
    let size = in.params.xy;
    let r = size * 0.5;
    let band = in.params.w >= 2.0;
    let ellipse = in.params.w - select(0.0, 2.0, band) > 0.5;
    let shrink = in.grad.x;
    let grow = in.grad.y;

    let plain = select(
        rect_distance(in.local, size, in.params.z),
        ellipse_distance(in.local, r, r),
        ellipse,
    );
    // A rounded rect's grown and shrunk outlines are its own distance
    // moved; an ellipse's are ellipses of other radii, which are
    // different curves — and once one has shrunk to nothing the band
    // reaches all the way in.
    let grown = r + vec2f(grow, grow);
    let outer = select(
        plain - grow,
        ellipse_distance(in.local, r, grown),
        ellipse,
    );
    let shrunk = r - vec2f(shrink, shrink);
    let inner = select(
        plain + shrink,
        select(ellipse_distance(in.local, r, max(shrunk, vec2f(1e-6, 1e-6))), 1e9, shrunk.x <= 0.0 || shrunk.y <= 0.0),
        ellipse,
    );
    let cov = edge(select(plain, outer, band));
    return select(cov, clamp(cov - edge(inner), 0.0, 1.0), band);
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4f {
    return in.color * coverage(in) * mask_cover(in.page, in.mask);
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
    @location(3) page: vec2f,
    @location(4) @interpolate(flat) mask: vec4f,
};

@vertex
fn vs_cover(
    @location(0) doc: vec2f,
    @location(1) local: vec2f,
    @location(3) color: vec4f,
    @location(4) grad: vec4f,
    @location(5) mask: vec4f,
) -> CoverOut {
    var out: CoverOut;
    out.pos = clip(doc);
    out.uv = local;
    out.color = color;
    out.grad = grad;
    out.page = doc;
    out.mask = mask;
    return out;
}

@fragment
fn fs_cover(in: CoverOut) -> @location(0) vec4f {
    return in.color * mask_cover(in.page, in.mask);
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
    @location(2) page: vec2f,
    @location(3) @interpolate(flat) mask: vec4f,
    // Which blend mode brings this down onto what is under it, for the
    // one quad that lays an isolated layer's surface back on the page —
    // or, for an adjustment layer's quad, which adjustment it is and
    // what it was asked for.
    @location(4) @interpolate(flat) mode: f32,
    @location(5) @interpolate(flat) params: vec4f,
    @location(6) @interpolate(flat) grad: vec4f,
    /// Three more numbers, for the one adjustment with more to say than
    /// the two above can carry.
    @location(7) @interpolate(flat) extra: vec3f,
};

@group(1) @binding(0) var image: texture_2d<f32>;
@group(1) @binding(1) var image_sampler: sampler;

@vertex
fn vs_image(
    @location(0) doc: vec2f,
    @location(1) uv: vec2f,
    @location(2) params: vec4f,
    @location(3) color: vec4f,
    @location(4) grad: vec4f,
    @location(5) mask: vec4f,
) -> ImageOut {
    var out: ImageOut;
    out.pos = clip(doc);
    out.uv = uv;
    out.alpha = color.a;
    out.page = doc;
    out.mask = mask;
    out.mode = params.x;
    out.params = params;
    out.grad = grad;
    out.extra = color.rgb;
    return out;
}

@fragment
fn fs_image(in: ImageOut) -> @location(0) vec4f {
    return textureSample(image, image_sampler, in.uv)
        * in.alpha
        * mask_cover(in.page, in.mask);
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
    return in.color * select(0.0, cov, inked) * mask_cover(in.page, in.mask);
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
    return ramp_color(ramp_at(uv, in.grad, in.color.r > 0.5))
        * in.color.a
        * cov
        * mask_cover(in.page, in.mask);
}

// A gradient-filled path: the stencil already said where the fill
// reached, so the cover quad only has to say what colour it is. Its
// corners carry the normalized box coordinates, which interpolate
// across the quad however the layer is transformed.
@fragment
fn fs_cover_gradient(in: CoverOut) -> @location(0) vec4f {
    return ramp_color(ramp_at(in.uv, in.grad, in.color.r > 0.5))
        * in.color.a
        * mask_cover(in.page, in.mask);
}

// A layer that composites with a blend mode is drawn on a surface of its
// own and brought down here: the surface is the source, a copy of what
// was already on the page is the backdrop, and the two are brought
// together the way the W3C compositing spec says — on the values a
// device shows rather than in linear light, which is the same choice the
// CPU renderer made and what makes a page look the same in the engine as
// in the SVG and PDF it exports.
@group(3) @binding(0) var backdrop: texture_2d<f32>;
@group(3) @binding(1) var backdrop_sampler: sampler;

fn to_shown(v: f32) -> f32 {
    if v <= 0.0031308 {
        return v * 12.92;
    }
    return 1.055 * pow(v, 1.0 / 2.4) - 0.055;
}

fn to_light(v: f32) -> f32 {
    if v <= 0.04045 {
        return v / 12.92;
    }
    return pow((v + 0.055) / 1.055, 2.4);
}

// Premultiplied linear to straight, shown values: what a blend reads.
fn shown3(c: vec3f, a: f32) -> vec3f {
    if a <= 0.0 {
        return vec3f(0.0, 0.0, 0.0);
    }
    let s = clamp(c / a, vec3f(0.0), vec3f(1.0));
    return vec3f(to_shown(s.r), to_shown(s.g), to_shown(s.b));
}

fn screen1(s: f32, d: f32) -> f32 {
    return s + d - s * d;
}

fn hard_light1(s: f32, d: f32) -> f32 {
    if s <= 0.5 {
        return d * 2.0 * s;
    }
    return screen1(2.0 * s - 1.0, d);
}

fn soft_light1(s: f32, d: f32) -> f32 {
    var dd = sqrt(d);
    if d <= 0.25 {
        dd = ((16.0 * d - 12.0) * d + 4.0) * d;
    }
    if s <= 0.5 {
        return d - (1.0 - 2.0 * s) * d * (1.0 - d);
    }
    return d + (2.0 * s - 1.0) * (dd - d);
}

fn dodge1(s: f32, d: f32) -> f32 {
    if d <= 0.0 {
        return 0.0;
    }
    if s >= 1.0 {
        return 1.0;
    }
    return min(d / (1.0 - s), 1.0);
}

fn burn1(s: f32, d: f32) -> f32 {
    if d >= 1.0 {
        return 1.0;
    }
    if s <= 0.0 {
        return 0.0;
    }
    return 1.0 - min((1.0 - d) / s, 1.0);
}

// W3C's own weights for the four that take one part of a colour and
// leave the rest — not the renderer's luminance, because the spec says
// so and matching it is what keeps the engine and the exporters agreeing.
fn w3c_lum(c: vec3f) -> f32 {
    return 0.3 * c.r + 0.59 * c.g + 0.11 * c.b;
}

fn clip_colour(c: vec3f) -> vec3f {
    let l = w3c_lum(c);
    let n = min(c.r, min(c.g, c.b));
    let x = max(c.r, max(c.g, c.b));
    var out = c;
    if n < 0.0 && l - n > 1e-6 {
        out = vec3f(l) + (out - vec3f(l)) * l / (l - n);
    }
    if x > 1.0 && x - l > 1e-6 {
        out = vec3f(l) + (out - vec3f(l)) * (1.0 - l) / (x - l);
    }
    return out;
}

fn set_lum(c: vec3f, l: f32) -> vec3f {
    return clip_colour(c + vec3f(l - w3c_lum(c)));
}

fn saturation_of(c: vec3f) -> f32 {
    return max(c.r, max(c.g, c.b)) - min(c.r, min(c.g, c.b));
}

// Stretch a colour's channels to a given saturation, keeping which
// channel is which: the middle one lands where it sat between the two
// others.
fn set_sat(c: vec3f, s: f32) -> vec3f {
    let hi = max(c.r, max(c.g, c.b));
    let lo = min(c.r, min(c.g, c.b));
    if hi <= lo {
        return vec3f(0.0, 0.0, 0.0);
    }
    let mid = (c.r + c.g + c.b) - hi - lo;
    let scaled = (mid - lo) * s / (hi - lo);
    // Put the three back where they came from, by value.
    var out = vec3f(0.0, 0.0, 0.0);
    out.r = select(select(scaled, s, c.r == hi), 0.0, c.r == lo);
    out.g = select(select(scaled, s, c.g == hi), 0.0, c.g == lo);
    out.b = select(select(scaled, s, c.b == hi), 0.0, c.b == lo);
    return out;
}

fn blended(mode: i32, s: vec3f, d: vec3f) -> vec3f {
    switch mode {
        case 1: { return s * d; }
        case 2: { return vec3f(screen1(s.r, d.r), screen1(s.g, d.g), screen1(s.b, d.b)); }
        case 3: { return vec3f(hard_light1(d.r, s.r), hard_light1(d.g, s.g), hard_light1(d.b, s.b)); }
        case 4: { return min(s, d); }
        case 5: { return max(s, d); }
        case 6: { return vec3f(dodge1(s.r, d.r), dodge1(s.g, d.g), dodge1(s.b, d.b)); }
        case 7: { return vec3f(burn1(s.r, d.r), burn1(s.g, d.g), burn1(s.b, d.b)); }
        case 8: { return vec3f(hard_light1(s.r, d.r), hard_light1(s.g, d.g), hard_light1(s.b, d.b)); }
        case 9: { return vec3f(soft_light1(s.r, d.r), soft_light1(s.g, d.g), soft_light1(s.b, d.b)); }
        case 10: { return abs(s - d); }
        case 11: { return s + d - 2.0 * s * d; }
        case 12: { return set_lum(set_sat(s, saturation_of(d)), w3c_lum(d)); }
        case 13: { return set_lum(set_sat(d, saturation_of(s)), w3c_lum(d)); }
        case 14: { return set_lum(s, w3c_lum(d)); }
        case 15: { return set_lum(d, w3c_lum(s)); }
        default: { return s; }
    }
}

@fragment
fn fs_blend(in: ImageOut) -> @location(0) vec4f {
    let src = textureSampleLevel(image, image_sampler, in.uv, 0.0)
        * in.alpha
        * mask_cover(in.page, in.mask);
    let dst = textureSampleLevel(backdrop, backdrop_sampler, in.uv, 0.0);
    let sa = src.a;
    let da = dst.a;
    let b = clamp(blended(i32(in.mode), shown3(src.rgb, sa), shown3(dst.rgb, da)), vec3f(0.0), vec3f(1.0));
    let light = vec3f(to_light(b.r), to_light(b.g), to_light(b.b));
    // W3C compositing: (1-da)*s + (1-sa)*d + sa*da*B, all premultiplied.
    return vec4f(
        (1.0 - da) * src.rgb + (1.0 - sa) * dst.rgb + sa * da * light,
        sa + da * (1.0 - sa),
    );
}

// An adjustment layer rewrites everything composited below it, weighted
// by its own opacity and by its mask. It reads what is under it the way
// a blend does — from a copy taken before the pass — and writes the
// answer over what was there.
//
// The arithmetic is the CPU renderer's, arm for arm: some of it works in
// linear light and some on the values a device shows, and which is which
// is a decision that belongs to the adjustment rather than to the
// renderer drawing it.
fn adjusted(kind: i32, p: vec4f, q: vec4f, c: vec3f) -> vec3f {
    switch kind {
        // Exposure: stops, which is a gain.
        case 1: {
            return c * pow(2.0, p.y);
        }
        // Brightness and contrast, about the middle.
        case 2: {
            return clamp((c + vec3f(p.y) - vec3f(0.5)) * (1.0 + p.z) + vec3f(0.5), vec3f(0.0), vec3f(1.0));
        }
        // Hue rotation (the feColorMatrix one), then saturation about
        // the pixel's own luminance, then a lightness offset.
        case 3: {
            let a = radians(p.y);
            let sn = sin(a);
            let cs = cos(a);
            let m0 = vec3f(0.213 + cs * 0.787 - sn * 0.213, 0.715 - cs * 0.715 - sn * 0.715, 0.072 - cs * 0.072 + sn * 0.928);
            let m1 = vec3f(0.213 - cs * 0.213 + sn * 0.143, 0.715 + cs * 0.285 + sn * 0.140, 0.072 - cs * 0.072 - sn * 0.283);
            let m2 = vec3f(0.213 - cs * 0.213 - sn * 0.787, 0.715 - cs * 0.715 + sn * 0.715, 0.072 + cs * 0.928 + sn * 0.072);
            let turned = vec3f(dot(m0, c), dot(m1, c), dot(m2, c));
            let l = dot(vec3f(0.2126, 0.7152, 0.0722), turned);
            return clamp(vec3f(l) + (turned - vec3f(l)) * (1.0 + p.z) + vec3f(p.w), vec3f(0.0), vec3f(1.0));
        }
        // Levels: an input range, a gamma, an output range.
        case 4: {
            let span = max(p.z - p.y, 1e-3);
            let e = 1.0 / max(p.w, 0.05);
            let v = pow(clamp((c - vec3f(p.y)) / span, vec3f(0.0), vec3f(1.0)), vec3f(e));
            return clamp(vec3f(q.x) + v * (q.y - q.x), vec3f(0.0), vec3f(1.0));
        }
        // White balance: a gain per channel, half the slider's travel at
        // each end so the extremes still hold a picture.
        case 5: {
            let warm = clamp(p.y, -1.0, 1.0) * 0.5;
            let mag = clamp(p.z, -1.0, 1.0) * 0.5;
            return clamp(c * vec3f(1.0 + warm, 1.0 - mag, 1.0 - warm), vec3f(0.0), vec3f(1.0));
        }
        // Vibrance: saturation weighted by how much colour there is
        // already, measured as a fraction of the pixel's own brightness.
        case 6: {
            let l = dot(vec3f(0.2126, 0.7152, 0.0722), c);
            let top = max(c.r, max(c.g, c.b));
            var sat = 0.0;
            if top > 1e-6 {
                sat = (top - min(c.r, min(c.g, c.b))) / top;
            }
            let s = 1.0 + p.y * (1.0 - clamp(sat, 0.0, 1.0));
            return clamp(vec3f(l) + (c - vec3f(l)) * s, vec3f(0.0), vec3f(1.0));
        }
        // Black and white: a recipe, normalized by its own total.
        case 7: {
            var w = vec3f(p.y, p.z, p.w);
            let total = w.r + w.g + w.b;
            if abs(total) < 1e-4 {
                w = vec3f(0.2126, 0.7152, 0.0722);
            } else {
                w = w / total;
            }
            return vec3f(clamp(dot(c, w), 0.0, 1.0));
        }
        // Inverted on the values a device shows: linear 0.5 shows as 188
        // and would come back a near-black rather than itself.
        case 8: {
            let k = clamp(p.y, 0.0, 1.0);
            let s = vec3f(to_shown(clamp(c.r, 0.0, 1.0)), to_shown(clamp(c.g, 0.0, 1.0)), to_shown(clamp(c.b, 0.0, 1.0)));
            let f = s + (vec3f(1.0) - s - s) * k;
            return vec3f(to_light(f.r), to_light(f.g), to_light(f.b));
        }
        // Shadows and highlights: each end pulls as the cube of the
        // distance from the other, and what moves is the brightness —
        // the colour comes along with it.
        case 9: {
            let l = clamp(dot(vec3f(0.2126, 0.7152, 0.0722), c), 0.0, 1.0);
            let s = to_shown(l);
            let lo = clamp(p.y, -1.0, 1.0);
            let hi = clamp(p.z, -1.0, 1.0);
            let moved = 0.5 * (lo * (1.0 - s) * (1.0 - s) * (1.0 - s) - hi * s * s * s);
            let want = to_light(clamp(s + moved, 0.0, 1.0));
            if l <= 1e-6 {
                return vec3f(want);
            }
            return c * (want / l);
        }
        default: {
            return c;
        }
    }
}

// A table an adjustment is stated by, read the way the CPU reads its
// own: the first and last texels are the ends, so a value maps onto
// their centres and the sampler fills in between.
fn table_at(t: f32) -> vec4f {
    let n = f32(textureDimensions(image).x);
    let u = (clamp(t, 0.0, 1.0) * (n - 1.0) + 0.5) / n;
    return textureSampleLevel(image, image_sampler, vec2f(u, 0.5), 0.0);
}

// A colour's hue, saturation and lightness, with the hue in sixths of
// the wheel — red at 0, yellow at 1, round to magenta at 5 — which is
// the order a colour panel's bands are always in, and what the bands of
// a selective adjustment are numbered by.
fn to_hsl(c: vec3f) -> vec3f {
    let hi = max(c.r, max(c.g, c.b));
    let lo = min(c.r, min(c.g, c.b));
    let light = (hi + lo) * 0.5;
    let chroma = hi - lo;
    if chroma <= 1e-6 {
        return vec3f(0.0, 0.0, light);
    }
    let sat = clamp(chroma / max(1.0 - abs(2.0 * light - 1.0), 1e-6), 0.0, 1.0);
    var hue = (c.r - c.g) / chroma + 4.0;
    if hi == c.r {
        hue = (c.g - c.b) / chroma;
    } else if hi == c.g {
        hue = (c.b - c.r) / chroma + 2.0;
    }
    return vec3f(hue - 6.0 * floor(hue / 6.0), sat, light);
}

fn from_hsl(hsl: vec3f) -> vec3f {
    let chroma = (1.0 - abs(2.0 * hsl.z - 1.0)) * hsl.y;
    let h = hsl.x - 6.0 * floor(hsl.x / 6.0);
    let x = chroma * (1.0 - abs((h - 2.0 * floor(h / 2.0)) - 1.0));
    let sixth = i32(h);
    var rgb = vec3f(chroma, 0.0, x);
    switch sixth {
        case 0: { rgb = vec3f(chroma, x, 0.0); }
        case 1: { rgb = vec3f(x, chroma, 0.0); }
        case 2: { rgb = vec3f(0.0, chroma, x); }
        case 3: { rgb = vec3f(0.0, x, chroma); }
        case 4: { rgb = vec3f(x, 0.0, chroma); }
        default: {}
    }
    return clamp(rgb + vec3f(hsl.z - chroma * 0.5), vec3f(0.0), vec3f(1.0));
}

// The four stated by a table or in bands of colour, which have more to
// say than the vertex can carry: the table is bound where a picture's
// pixels would be.
fn adjusted_from_table(kind: i32, p: vec4f, q: vec4f, e: vec3f, c: vec3f) -> vec3f {
    switch kind {
        // Curves: a master curve every channel goes through, and a curve
        // of its own for each after it, on the values a device shows. A
        // channel that was never drawn carries the straight line, so
        // there is nothing to ask about.
        case 10: {
            let shown = vec3f(to_shown(clamp(c.r, 0.0, 1.0)), to_shown(clamp(c.g, 0.0, 1.0)), to_shown(clamp(c.b, 0.0, 1.0)));
            let master = vec3f(table_at(shown.r).r, table_at(shown.g).r, table_at(shown.b).r);
            let own = vec3f(table_at(master.r).g, table_at(master.g).b, table_at(master.b).a);
            return vec3f(to_light(own.r), to_light(own.g), to_light(own.b));
        }
        // A gradient map: every tone replaced by the colour at its own
        // place along a ramp. Where a tone sits is its brightness as a
        // device shows it — the middle of the ramp should land on the
        // tones that look middling, and linear light's middle shows as a
        // light grey. The ramp's colours are premultiplied by their own
        // alpha; the pixel keeps the alpha it had, so that is divided
        // back out.
        case 11: {
            let lum = to_shown(clamp(dot(vec3f(0.2126, 0.7152, 0.0722), c), 0.0, 1.0));
            let ramp = table_at(lum);
            if ramp.a <= 0.0 {
                return c;
            }
            return ramp.rgb / ramp.a;
        }
        // Hue, saturation and lightness asked of one band of colour at a
        // time. A pixel belongs to the bands its own hue falls between,
        // by how near it is to each; the weights are a triangle a band
        // wide, so they add to one and no colour sits in a seam.
        case 12: {
            let shown = vec3f(to_shown(clamp(c.r, 0.0, 1.0)), to_shown(clamp(c.g, 0.0, 1.0)), to_shown(clamp(c.b, 0.0, 1.0)));
            let hsl = to_hsl(shown);
            var d = vec3f(0.0);
            for (var i = 0; i < 6; i = i + 1) {
                let away = abs(hsl.x - f32(i));
                let w = max(1.0 - min(away, 6.0 - away), 0.0);
                d = d + w * table_at(f32(i) / 5.0).rgb;
            }
            if hsl.y <= 1e-4 || (d.r == 0.0 && d.g == 0.0 && d.b == 0.0) {
                return c;
            }
            // How much of the change a pixel takes is how much colour it
            // has, though a third of full saturation already takes all
            // of it: a pale sky is still a sky.
            let take = clamp(hsl.y * 3.0, 0.0, 1.0);
            let moved = from_hsl(vec3f(
                hsl.x + d.r * 0.5 * take,
                clamp(hsl.y * (1.0 + d.g * take), 0.0, 1.0),
                clamp(hsl.z + d.b * 0.5 * take, 0.0, 1.0),
            ));
            return vec3f(to_light(moved.r), to_light(moved.g), to_light(moved.b));
        }
        // Colour balance: the three ranges of tone pushed along the
        // three opponent pairs, the range read from the pixel's own
        // lightness so a shift moves a colour rather than pulling it
        // apart.
        case 13: {
            let lo3 = vec3f(p.y, p.z, p.w);
            let mid3 = vec3f(q.x, q.y, q.z);
            let hi3 = vec3f(q.w, e.x, e.y);
            if all(lo3 == vec3f(0.0)) && all(mid3 == vec3f(0.0)) && all(hi3 == vec3f(0.0)) {
                return c;
            }
            let shown = vec3f(to_shown(clamp(c.r, 0.0, 1.0)), to_shown(clamp(c.g, 0.0, 1.0)), to_shown(clamp(c.b, 0.0, 1.0)));
            let light = (max(shown.r, max(shown.g, shown.b)) + min(shown.r, min(shown.g, shown.b))) * 0.5;
            let ramp = 0.25;
            let edge = 0.333;
            let scale = 0.7;
            let wlo = clamp((light - edge) / -ramp + 0.5, 0.0, 1.0) * scale;
            let whi = clamp((light + edge - 1.0) / ramp + 0.5, 0.0, 1.0) * scale;
            let wmid = clamp((light - edge) / ramp + 0.5, 0.0, 1.0)
                * clamp((light + edge - 1.0) / -ramp + 0.5, 0.0, 1.0)
                * scale;
            let moved = clamp(
                shown + wlo * clamp(lo3, vec3f(-1.0), vec3f(1.0))
                    + wmid * clamp(mid3, vec3f(-1.0), vec3f(1.0))
                    + whi * clamp(hi3, vec3f(-1.0), vec3f(1.0)),
                vec3f(0.0),
                vec3f(1.0),
            );
            var out = moved;
            if e.z > 0.5 {
                // The colour that was asked for, at the lightness the
                // pixel already had.
                let now = to_hsl(moved);
                out = from_hsl(vec3f(now.x, now.y, to_hsl(shown).z));
            }
            return vec3f(to_light(out.r), to_light(out.g), to_light(out.b));
        }
        default: {
            return c;
        }
    }
}

@fragment
fn fs_adjust(in: ImageOut) -> @location(0) vec4f {
    let was = textureSampleLevel(backdrop, backdrop_sampler, in.uv, 0.0);
    let weight = in.alpha * mask_cover(in.page, in.mask);
    if weight <= 0.0 || was.a <= 0.0 {
        return was;
    }
    let kind = i32(in.mode);
    if kind >= 14 {
        return filtered(kind, in.params, in.grad, in.page, was, weight);
    }
    // Straight alpha in, premultiplied out, which is where the
    // adjustments are stated.
    let straight = was.rgb / was.a;
    var out = adjusted(kind, in.params, in.grad, straight);
    if kind >= 10 {
        out = adjusted_from_table(kind, in.params, in.grad, in.extra, straight);
    }
    return vec4f(mix(was.rgb, out * was.a, weight), was.a);
}

// The two filters that are a function of one pixel and of where that
// pixel is on the page — the rest read a neighbourhood, which wants
// passes of its own, and are still the CPU's.
//
// Both are stated on the premultiplied value the surface already holds,
// and both fold the layer's weight into their own strength rather than
// mixing the result against the original. That is what the CPU does,
// and the two are not the same reading: a grain shift that clips is
// clipped after it has been weakened, not before.
fn filtered(kind: i32, p: vec4f, g: vec4f, at: vec2f, was: vec4f, weight: f32) -> vec4f {
    if kind == 14 {
        // Measured from the page's own middle in the page's own units,
        // so panning slides the picture under the darkening rather than
        // carrying the darkening along with it.
        let mid = page.size * 0.5;
        let far = max(length(mid), 1e-3);
        let inner = clamp(p.z, 0.0, 0.999);
        let ease = clamp(p.w, 0.0, 1.0);
        let d = length(at - mid) / far;
        let t = clamp((d - inner) / (1.0 - inner), 0.0, 1.0);
        // Softness eases the shoulder: none of it is a straight ramp
        // from where it begins, all of it a curve with no edge anywhere.
        let fall = t + (t * t * (3.0 - 2.0 * t) - t) * ease;
        let gain = max(1.0 - p.y * weight * fall, 0.0);
        // Premultiplied, so scaling the three channels and leaving
        // alpha alone darkens the colour without touching what is or is
        // not covered.
        return vec4f(was.rgb * gain, was.a);
    }
    // Grain. One speck is `p.z` page pixels across, so it grows with the
    // picture rather than staying the size of a screen pixel, and it is
    // a function of where the speck is and of the seed alone — nothing
    // carried from the pixel before — which is what lets it be a live
    // layer rather than something baked once.
    let w = p.y * weight;
    let cell = vec2i(floor(at / max(p.z, 1e-3)));
    let seed = u32(g.x) + u32(g.y) * 65536u;
    let one = (speck(cell, seed) - 0.5) * w;
    // Every channel moved together is film grain; each moved on its own
    // is a sensor's noise.
    var shift = vec3f(one, one, one);
    if p.w == 0.0 {
        shift = vec3f(one, (speck(cell, seed + 1u) - 0.5) * w, (speck(cell, seed + 2u) - 0.5) * w);
    }
    // Premultiplied, so a shift is a share of the pixel's own alpha and
    // a clear pixel stays clear.
    return vec4f(clamp(was.rgb + shift * was.a, vec3f(0.0), vec3f(was.a)), was.a);
}

// A cheap integer hash: multiply, mix the halves, repeat. Good enough
// that neighbouring cells look unrelated, which is all grain asks. The
// CPU renderer's, arithmetic for arithmetic, so both grain the same page
// the same way.
fn speck(cell: vec2i, seed: u32) -> f32 {
    var h = (bitcast<u32>(cell.x) * 0x8da6b343u) ^ (bitcast<u32>(cell.y) * 0xd8163841u) ^ seed;
    h = h ^ (h >> 15u);
    h = h * 0x2c1b3c6du;
    h = h ^ (h >> 12u);
    h = h * 0x29715aebu;
    h = h ^ (h >> 16u);
    return f32(h) / 4294967295.0;
}

// One box-blur pass along an axis: the average of the 2r+1 texels
// centred on this one, taken from the texture bound as an image.
//
// Three of these each way approximate a Gaussian, which is the CPU
// renderer's blur and so is this one's — same radius, same passes, same
// order. The taps land on texel centres, so linear sampling reads one
// texel exactly; the sampler clamps at the edge, which is what the CPU's
// running window does when it runs off the end of a lane.
@fragment
fn fs_box(in: ImageOut) -> @location(0) vec4f {
    let radius = i32(in.params.x);
    let vertical = in.params.y != 0.0;
    // What the window finds when it runs off the end of the line.
    // Repeating the edge is right for a blur *filter*, which reads what
    // is under it and past which the picture goes on; nothing is right
    // for a live effect's field, which is built over the layer's own box
    // grown by the effect's reach and is nothing past it. The two only
    // part company where the surface cut that box short — a layer near
    // the page's edge — and there repeating the edge invents silhouette
    // and casts a heavier shadow for it. `params.z` says which.
    let nothing = in.params.z != 0.0;
    let size = vec2i(page.size);
    let lo = vec2i(page.lo);
    let hi = vec2i(page.hi) - vec2i(1, 1);
    let here = vec2i(floor(in.uv * page.size));
    var sum = vec4f(0.0, 0.0, 0.0, 0.0);
    for (var i = -radius; i <= radius; i = i + 1) {
        var p = here;
        if vertical {
            p.y = p.y + i;
        } else {
            p.x = p.x + i;
        }
        if nothing {
            if p.x >= 0 && p.y >= 0 && p.x < size.x && p.y < size.y {
                sum = sum + textureLoad(image, p, 0);
            }
        } else {
            sum = sum + textureLoad(image, clamp(p, lo, hi), 0);
        }
    }
    return sum / f32(2 * radius + 1);
}

// A smear along a line: the average of `params.x` taps, `params.yz`
// apart, centred on this pixel.
//
// One pass rather than the blur's six, and not along an axis: the line
// runs at whatever angle it was given, so there is nothing to separate
// into a horizontal turn and a vertical one. The taps are the CPU
// renderer's — the same count, the same spacing, each rounded to a whole
// pixel and clamped at the page's edge, which is what its own reading
// past the region does — so the two come out at the same picture rather
// than at two plausible ones.
//
// `floor(v + 0.5)` rather than `round`, which in WGSL takes a half to
// the even neighbour where the CPU takes it away from zero.
@fragment
fn fs_smear(in: ImageOut) -> @location(0) vec4f {
    let n = i32(in.params.x);
    let step = vec2f(in.params.y, in.params.z);
    let half = f32(n / 2);
    let here = floor(in.uv * page.size);
    var sum = vec4f(0.0, 0.0, 0.0, 0.0);
    for (var k = 0; k < n; k = k + 1) {
        let at = floor(here + step * (f32(k) - half) + vec2f(0.5, 0.5));
        let p = clamp(at, page.lo, page.hi - vec2f(1.0, 1.0));
        sum = sum + textureLoad(image, vec2i(p), 0);
    }
    return sum / f32(n);
}

// Where along a segment a point falls, from nought at one end to one at
// the other and clamped to the segment, and how far it is from it. The
// CPU renderer's own two, written the same way round: a brush is an
// analytic coverage rather than a rasterized shape, so a difference in
// the arithmetic here is a difference in every edge the brush lays.
fn seg_parameter(p: vec2f, a: vec2f, b: vec2f) -> f32 {
    let d = b - a;
    let len2 = d.x * d.x + d.y * d.y;
    if len2 <= 1e-12 {
        return 0.0;
    }
    return clamp(((p.x - a.x) * d.x + (p.y - a.y) * d.y) / len2, 0.0, 1.0);
}

fn seg_distance(p: vec2f, a: vec2f, b: vec2f) -> f32 {
    let d = b - a;
    let t = seg_parameter(p, a, b);
    let c = a + d * t;
    return sqrt((p.x - c.x) * (p.x - c.x) + (p.y - c.y) * (p.y - c.y));
}

// One segment of a brush stroke: a round-capped band from one point to
// the next, as wide as the radius at each end says and fading across the
// softness the brush was set to.
//
// `params` is the segment's two ends in the layer's own space, which is
// what `local` interpolates to; `grad` is the radius at each end, the
// softness, and the width of the narrowest fade there can be — one
// device pixel, so a hard brush has an antialiased edge for the same
// reason every other edge here does.
//
// Written into a texture of its own with max blending, because the
// segments of one stroke *union*: a stroke that doubles back is not
// darker where it crossed itself, which is what taking the most any
// segment lays says and what adding them up would not.
@fragment
fn fs_brush(in: VsOut) -> @location(0) vec4f {
    let a = vec2f(in.params.x, in.params.y);
    let b = vec2f(in.params.z, in.params.w);
    let along = seg_parameter(in.local, a, b);
    let r = in.grad.x + (in.grad.y - in.grad.x) * along;
    if r <= 0.0 {
        return vec4f(0.0, 0.0, 0.0, 0.0);
    }
    let fade = max(r * in.grad.z, in.grad.w);
    let c = clamp((r - seg_distance(in.local, a, b)) / fade, 0.0, 1.0);
    return vec4f(c, c, c, c);
}

// A brush stroke coming down on the layer: the coverage its segments
// left, in the colour it lays. An eraser hands down the same coverage in
// nothing at all and is brought down by a blend that takes it off.
@fragment
fn fs_paint(in: ImageOut) -> @location(0) vec4f {
    let c = textureSampleLevel(image, image_sampler, in.uv, 0.0).a;
    // A stroke laid inside a region carries that region, and it rides
    // the same slot a layer's mask does — one coverage a stroke is held
    // back by, read once.
    return in.grad * c * mask_cover(in.page, in.mask);
}

// A clone stroke coming down: the coverage its segments left, filled
// with what the surface already holds a fixed distance away.
//
// The offset is a direction rather than a place — the source written in
// the layer's own space, carried through the layer's transform without
// its translation — and the read is the nearest whole pixel, since what
// is being lifted is pixels rather than a picture with edges to
// resample. Off the surface there is nothing to clone, and nothing is
// what gets laid.
//
// What is under the stroke and what is being lifted are the same
// texture, and they have to be: the CPU renderer takes its copy before
// the stroke lays anything, so a stroke running over its own source
// reads what was there rather than what it has just painted. Which
// means the blend has what it needs too, and the fragment can work the
// whole composite out and write over what was there.
//
// `params.yz` is the offset in device pixels, `params.x` the blend mode.
@fragment
fn fs_clone(in: ImageOut) -> @location(0) vec4f {
    let cover = textureSampleLevel(image, image_sampler, in.uv, 0.0).a
        * in.alpha
        * mask_cover(in.page, in.mask);
    let dst = textureSampleLevel(backdrop, backdrop_sampler, in.uv, 0.0);
    if cover <= 0.0 {
        return dst;
    }
    // The CPU renderer rounds `px + s`, where `px` is the pixel's index
    // and not its centre — so the half a pixel that `uv` carries is the
    // half that `round` adds, and the two cancel. `floor` rather than
    // WGSL's `round`, which takes a half to the even neighbour where the
    // CPU takes it away from zero.
    let at = floor(in.uv * page.size + vec2f(in.params.y, in.params.z));
    if at.x < 0.0 || at.y < 0.0 || at.x >= page.size.x || at.y >= page.size.y {
        return dst;
    }
    let lifted = textureLoad(backdrop, vec2i(at), 0);
    if lifted.a <= 0.0 {
        return dst;
    }
    let src = lifted * cover;
    let sa = src.a;
    let da = dst.a;
    let b = clamp(blended(i32(in.mode), shown3(src.rgb, sa), shown3(dst.rgb, da)), vec3f(0.0), vec3f(1.0));
    let light = vec3f(to_light(b.r), to_light(b.g), to_light(b.b));
    return vec4f(
        (1.0 - da) * src.rgb + (1.0 - sa) * dst.rgb + sa * da * light,
        sa + da * (1.0 - sa),
    );
}

// A layer's silhouette in one flat colour, or the hole around it: what
// every live effect is built from, and the reason a shadow of a
// photograph is a shape rather than a picture of one.
//
// `params.x` is one to take the hole instead of the layer — an inner
// shadow is cast from around the layer and then kept inside it. `grad`
// is the tint, premultiplied and already weighed by the effect's own
// opacity: tinting before the blur rather than after is the same answer,
// since the tint is constant and the blur is linear, and it means one
// texture instead of two.
@fragment
fn fs_field(in: ImageOut) -> @location(0) vec4f {
    // `alpha` is what the surface still owes the silhouette: a layer's
    // own opacity is inside the surface already, but a group's belongs
    // to the composite rather than to its children, so it is taken here
    // instead — the CPU renderer builds the same silhouette out of a
    // group's surface *after* fading it.
    let a = textureSampleLevel(image, image_sampler, in.uv, 0.0).a * in.alpha;
    // An outline is a distance from an edge, and an edge is where the
    // silhouette is half covered — so what a band is measured out from
    // is a yes or a no rather than a coverage, and `params.y` says at
    // what coverage the answer turns. It comes down as one, not as the
    // tint: what the band's own passes carry is a distance, and a
    // distance has no colour until it has been cut to a width.
    if in.params.x == 2.0 {
        return vec4f(select(0.0, 1.0, a >= in.params.y));
    }
    let cover = select(a, 1.0 - a, in.params.x != 0.0);
    return in.grad * cover;
}

// Whether the pixel at `p` is inside the silhouette the band is measured
// from. Off the surface is outside: the CPU renderer measures over a
// window it allocated, and there is nothing past the end of it either.
fn band_inside(p: vec2i, size: vec2i) -> bool {
    if p.x < 0 || p.y < 0 || p.x >= size.x || p.y >= size.y {
        return false;
    }
    return textureLoad(image, p, 0).a > 0.5;
}

// One pass of an outline's band.
//
// The band is how far a pixel is from the layer's silhouette, cut to the
// width the outline was asked for and feathered over the last pixel of
// it. The distance is a true Euclidean one — the same the CPU renderer
// measures, and the same a region is grown by — worked out the separable
// way, which is what makes it two passes rather than a search of the
// whole disc: down each column for how far the nearest inside pixel in
// that column is, then along each row taking the least of `dx² + g²`,
// which is exactly the distance to the nearest inside pixel anywhere.
//
// Neither pass looks further than the band reaches, and anything it did
// not look at is further off than that — so the cap cannot decide an
// answer inside the band, only outside it, where the answer is nothing.
//
// `params.x` is the width in device pixels; `params.y` says which pass.
// `grad` is the tint, premultiplied and already weighed by the effect's
// own opacity, which the second pass lays the band down in.
@fragment
fn fs_band(in: ImageOut) -> @location(0) vec4f {
    let width = in.params.x;
    let far = ceil(width) + 1.0;
    let n = i32(far);
    let size = vec2i(page.size);
    let here = vec2i(floor(in.uv * page.size));
    if in.params.y == 0.0 {
        if band_inside(here, size) {
            return vec4f(0.0);
        }
        var best = far;
        for (var k = 1; k <= n; k = k + 1) {
            if band_inside(vec2i(here.x, here.y - k), size)
                || band_inside(vec2i(here.x, here.y + k), size) {
                best = f32(k);
                break;
            }
        }
        return vec4f(best);
    }
    var best = far * far;
    for (var k = -n; k <= n; k = k + 1) {
        let x = here.x + k;
        if x < 0 || x >= size.x {
            continue;
        }
        let g = textureLoad(image, vec2i(x, here.y), 0).r;
        best = min(best, f32(k) * f32(k) + g * g);
    }
    // The distance counts from pixel centre to pixel centre, and the
    // centre of an edge pixel already sits half a pixel inside the
    // shape — so the distance to the edge itself is one less half at
    // each end. The CPU renderer's own arithmetic, written the same way
    // round.
    return in.grad * clamp(width + 1.0 - sqrt(best), 0.0, 1.0);
}

// A field read at the effect's offset, in the window it was built over.
//
// That window is the whole surface here, and the CPU renderer's is the
// layer's box grown by how far the effect reaches — but the field is
// nothing at the edge of *that* box, so the two only part company where
// the surface itself cuts the layer short. There they have to part
// company the same way: what falls off the window is nothing, rather
// than its edge repeated. A shape hanging off the top of the page casts
// no shadow back onto the first row, and a clamped read would have given
// it one — and a layer near the page's edge would take a heavier shadow
// than the same layer in the middle, which is a shadow that is not a
// function of the layer alone. The four taps of the bilinear read are
// taken one at a time so that a tap off the window is nothing rather
// than the edge repeated, which is what the CPU renderer's own four do.
fn field_tap(p: vec2i, size: vec2i) -> vec4f {
    if p.x < 0 || p.y < 0 || p.x >= size.x || p.y >= size.y {
        return vec4f(0.0);
    }
    return textureLoad(image, p, 0);
}

fn field_at(uv: vec2f, offset: vec2f) -> vec4f {
    let size = vec2i(page.size);
    let s = uv * page.size - vec2f(0.5, 0.5) - offset;
    let f = floor(s);
    let t = s - f;
    let i = vec2i(f);
    let top = mix(field_tap(i, size), field_tap(i + vec2i(1, 0), size), t.x);
    let low = mix(
        field_tap(i + vec2i(0, 1), size),
        field_tap(i + vec2i(1, 1), size),
        t.x,
    );
    return mix(top, low, t.y);
}

// The blurred field coming down onto what is under the layer, read at
// the effect's offset.
//
// The read is linear and clamped at the field's edge, which is the same
// bilinear reading with the same clamp the CPU renderer's stamp takes —
// and the field is nothing at its own edge, since it was built over a
// window grown by how far the effect reaches, so a clamped repeat is a
// repeat of nothing.
//
// `params.xy` is the offset in device pixels. `params.z` is one when the
// layer's own coverage is to be taken as well, which is what keeps an
// inner shadow inside the silhouette instead of spilling past it.
@fragment
fn fs_effect(in: ImageOut) -> @location(0) vec4f {
    var out = field_at(in.uv, vec2f(in.params.x, in.params.y));
    if in.params.z != 0.0 {
        // `params.w` is what the surface still owes its own silhouette,
        // which is a group's opacity and nothing for anything else.
        out = out * textureSampleLevel(backdrop, backdrop_sampler, in.uv, 0.0).a * in.params.w;
    }
    return out * in.alpha * mask_cover(in.page, in.mask);
}

// A field made ready to go down on its own: read at the effect's offset,
// and — for an inner shadow — held to the layer's own coverage. Those
// are the two things the stamp does as it lays a field down, and both
// want the texture a blend would want for what is under it. So when the
// layer carries a blend mode this pass does them first, on the scratch
// pair, and what the stamp is left with is an ordinary picture to bring
// down by the blend.
//
// `params.xy` is the offset in device pixels, `params.z` one when the
// layer's own coverage is to be taken as well; the layer's surface is
// bound where the backdrop goes, since a scratch pass has nothing under
// it to read.
@fragment
fn fs_settle(in: ImageOut) -> @location(0) vec4f {
    var out = field_at(in.uv, vec2f(in.params.x, in.params.y));
    if in.params.z != 0.0 {
        out = out * textureSampleLevel(backdrop, backdrop_sampler, in.uv, 0.0).a * in.params.w;
    }
    return out;
}

// Which block of a pixelate grid a pixel falls in, along one axis.
//
// The grid is laid out in the document rather than on the page, so the
// answer is where the pixel's centre lands once mapped back out of the
// space the filter sits in. Worked out here from the same numbers as the
// CPU renderer and in the same order, because a pixel that fell on one
// side of a block's edge there and the other side here would put a whole
// row in the wrong square.
fn block_of(p: f32, inv: f32, origin: f32, side: f32) -> f32 {
    return floor((inv * ((p + 0.5) - origin)) / side);
}

fn block_tap(here: vec2f, q: f32, vertical: bool) -> vec4f {
    var at = here;
    if vertical {
        at.y = q;
    } else {
        at.x = q;
    }
    return textureLoad(image, vec2i(at), 0);
}

// One pass of a pixelate along an axis: the average of the run of texels
// this one shares a block with.
//
// A block's average is separable where the grid is axis-aligned on the
// page — every pixel of a column is in the same column of blocks — so
// two of these are the whole filter, and the run each pass walks is
// found by asking `block_of` outward until the answer changes rather
// than by solving for the run's ends, which would be a rearrangement of
// the CPU's question rather than the question. Running off the page
// stops the walk, which is how a block hanging over the edge comes out
// as the average of the part inside it, exactly as it does there.
//
// `params` is the inverse scale along this axis, the origin it is
// measured from, the block's side in document units, and which axis.
@fragment
fn fs_block(in: ImageOut) -> @location(0) vec4f {
    let inv = in.params.x;
    let origin = in.params.y;
    let side = in.params.z;
    let vertical = in.params.w != 0.0;
    let here = floor(in.uv * page.size);
    let first = select(page.lo.x, page.lo.y, vertical);
    let limit = select(page.hi.x, page.hi.y, vertical);
    let p = select(here.x, here.y, vertical);
    let c = block_of(p, inv, origin, side);
    var sum = vec4f(0.0, 0.0, 0.0, 0.0);
    var n = 0.0;
    // Outward from this pixel both ways while the block holds. The
    // ceiling is a belt: the page is handed back before it is reached.
    var q = p;
    loop {
        if q < first || q >= limit || block_of(q, inv, origin, side) != c || p - q > 256.0 {
            break;
        }
        sum = sum + block_tap(here, q, vertical);
        n = n + 1.0;
        q = q - 1.0;
    }
    q = p + 1.0;
    loop {
        if q < first || q >= limit || block_of(q, inv, origin, side) != c || q - p > 256.0 {
            break;
        }
        sum = sum + block_tap(here, q, vertical);
        n = n + 1.0;
        q = q + 1.0;
    }
    return sum / max(n, 1.0);
}

// A blur layer coming back down: what was under it, and the blurred copy
// of it that the box passes left in a texture, weighed by the layer's
// opacity and its mask.
//
// `params.x` is zero for a plain blur, and for a sharpen it is how much
// of the difference between the two to add back — an unsharp mask, which
// is the same blur read as what the picture has too little of.
@fragment
fn fs_blur_down(in: ImageOut) -> @location(0) vec4f {
    let was = textureSampleLevel(backdrop, backdrop_sampler, in.uv, 0.0);
    let soft = textureSampleLevel(image, image_sampler, in.uv, 0.0);
    let weight = in.alpha * mask_cover(in.page, in.mask);
    if in.params.x == 0.0 {
        return mix(was, soft, weight);
    }
    // Premultiplied, so the channels are clamped to the alpha they are a
    // share of; the layer's weight goes into the amount rather than into
    // a mix, which is where the two readings differ once a channel
    // clips.
    let amount = in.params.x * weight;
    let out = clamp(was.rgb + (was.rgb - soft.rgb) * amount, vec3f(0.0), vec3f(max(was.a, 0.0)));
    return vec4f(out, was.a);
}
