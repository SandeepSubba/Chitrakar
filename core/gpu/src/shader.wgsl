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
    /// The document's own size, in the document's own units. Neither of
    /// the two above says it once a view scales anything, and a filter
    /// measured on the *picture* rather than on the surface needs it: a
    /// vignette is placed from the page's middle in page units, so
    /// panning slides the picture under the darkening.
    doc: vec2f,
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
    // Held to a few pixels either side of the rim, which is the only
    // place a distance decides anything: an edge a pixel wide reads
    // nothing from a distance of eight. It is held because the middle is
    // singular — the gradient goes to nothing there and this ratio to
    // minus infinity with it, and a number that large has a meaningless
    // rate of change across a pixel. That left a stray pixel about
    // three-quarters covered in the middle of a solid disc, which was
    // this backend's worst disagreement with the reference anywhere:
    // not an edge at all, and nothing an edge-drawing fix would reach.
    return clamp(f / max(length(g), 1e-6), -8.0, 8.0);
}

// How much of this pixel a signed distance covers. The soft band is one
// device pixel wide however the shape is transformed: fwidth measures
// the distance's own rate of change on the screen. Derivatives have to
// be taken in uniform control flow, which is why every distance below
// is computed and only then selected between.
// Exactly how much of a pixel a straight edge lets through: the area of
// a unit square on one side of a line. `d` is the signed distance to the
// edge in pixels, negative on the inside, and `n` the edge's unit normal
// in the same units.
//
// A ramp — half a pixel either side of the line, straight between — is
// this area only for an edge standing square on the page. Turn the edge
// and the square meets it corner-first, which the ramp cannot say: the
// area is quadratic over the part of the crossing where a corner is cut
// off and linear only over the part where two opposite sides are. Both
// pieces are below, and which is which is decided by how far apart the
// square's two corners on the line's own axis are. At forty-five degrees
// the linear piece vanishes and the whole crossing is the two triangles;
// square on, the quadratic piece vanishes and this is the ramp again.
fn half_plane(d: f32, n: vec2f) -> f32 {
    let a = min(abs(n.x), abs(n.y));
    let b = max(abs(n.x), abs(n.y));
    let t = abs(d);
    let far = (a + b) * 0.5;
    let near = (b - a) * 0.5;
    // A corner cut off the square: half the base times the height, in
    // units where the base and height are the distance divided by each
    // axis's share of the normal.
    let corner = (far - t) * (far - t) / max(2.0 * a * b, 1e-6);
    let straight = 0.5 - t / max(b, 1e-6);
    let outside = select(select(corner, straight, t < near), 0.0, t >= far);
    return select(1.0 - outside, outside, d >= 0.0);
}

fn edge(d: f32) -> f32 {
    // The gradient says both things this needs: how fast the distance
    // changes across a pixel, which turns it into pixels, and which way
    // the edge faces, which says how the pixel meets it.
    let g = vec2f(dpdx(d), dpdy(d));
    let len = max(length(g), 1e-6);
    return clamp(half_plane(d / len, g / len), 0.0, 1.0);
}

// How fast a quantity changes over one screen pixel, in the direction it
// changes fastest: the length of its screen-space gradient.
//
// `fwidth` is the cheaper |dx| + |dy|, which is the same number only when
// one of the two is zero. On an edge at forty-five degrees they differ by
// root two, so a ramp scaled by `fwidth` comes out that much too wide —
// and that is most of what separated this backend's ellipses from the
// reference's, whose spans are exact: the areas agreed to a third of a
// pixel while single pixels along the rim were out by a third of full
// scale.
fn across(v: f32) -> f32 {
    return max(length(vec2f(dpdx(v), dpdy(v))), 1e-6);
}

// How much of this pixel an axis-aligned box covers, given how far
// outside it this point is on each axis — negative inside. Exact for a
// box standing square on the page, since the area a box and a pixel share
// is the product of what they share along each axis on its own, and much
// the nearer answer for a turned one than a distance is.
fn box_cover(q: vec2f) -> f32 {
    return clamp(0.5 - q.x / across(q.x), 0.0, 1.0)
        * clamp(0.5 - q.y / across(q.y), 0.0, 1.0);
}

// How much of this pixel a box covers, given where this point is from
// the box's middle and how far the box reaches either side of it. What a
// pixel and a box share along one axis, times the same along the other —
// exact for a box standing square on the page, as `box_cover` is, and
// exact as well for one thinner than a pixel, which `box_cover` is not:
// its ramp is the overlap only while the box is at least a pixel across,
// and a pixel whose middle sat inside a line a third of a pixel wide came
// out two thirds covered rather than a third. Upright that never showed,
// since the middles of the pixels such a line crosses lie outside it;
// turned, half of them lie inside.
fn slab(p: f32, r: f32) -> f32 {
    let w = across(p);
    let lo = max(abs(p) - 0.5 * w, -r);
    let hi = min(abs(p) + 0.5 * w, r);
    return clamp((hi - lo) / w, 0.0, 1.0);
}

fn box_area(p: vec2f, r: vec2f) -> f32 {
    return slab(p.x, r.x) * slab(p.y, r.y);
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
    // A sharp-cornered rectangle is measured edge by edge rather than
    // by its distance. A distance is the same in every direction, so its
    // one-pixel band rounds a corner off: the pixel diagonally outside a
    // square's corner is three-quarters of a pixel from it and comes out
    // a tenth covered, where the area it actually covers is none. Two
    // half-planes multiplied is the exact area for a rectangle standing
    // square on the page and much the nearer answer for a turned one, and
    // it costs two clamps. Only where there is a corner to get wrong: a
    // rounded rect's corner really is an arc, and an ellipse has none.
    let q = abs(in.local - r) - r;
    // And its *band* is one such box less another: the outline grown and
    // the outline shrunk are both boxes, the shrunk one lies inside the
    // grown one, so the area between them is the difference of two exact
    // answers and is exact itself. Which mattered more than the fill did
    // — a square-cornered fill was already exact here and its stroke was
    // this backend's worst disagreement with the reference of any shape,
    // a fifth of full scale, because a band measured by a distance is
    // two roundings-off rather than one.
    // Away from the corners, that is. A band drawn round a corner is
    // carried round it by the join, and what the reference renderer
    // draws there is a quarter circle of the band's own reach — so the
    // outer outline is square along the runs and an arc at the four
    // places a run ends, and squaring it off there was worse than the
    // distance had been. The inner outline has no such reservation: the
    // inside of a corner is where two runs meet, not where one is
    // carried round.
    let sharp = !ellipse && in.params.z <= 0.0;
    let gone = shrunk.x <= 0.0 || shrunk.y <= 0.0;
    let round_off = q.x > 0.0 && q.y > 0.0 && grow > 1.0;
    let square_band = clamp(
        select(box_cover(q - vec2f(grow, grow)), edge(plain - grow), round_off)
            - select(box_cover(q + vec2f(shrink, shrink)), 0.0, gone),
        0.0,
        1.0,
    );
    let solid = select(edge(plain), box_area(in.local - r, r), sharp);
    let banded = select(
        clamp(edge(outer) - edge(inner), 0.0, 1.0),
        square_band,
        sharp,
    );
    return select(solid, banded, band);
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

// How much of this pixel the picture's box covers along one axis, where
// `u` runs 0..1 across it. The quad is drawn a device pixel wider than
// the box on every side, so the fragments either side of the border run
// too and this fades them: exactly the fraction of the pixel inside,
// the same number the CPU renderer computes as an area. Without it the
// border is whatever the 4-sample coverage mask happened to catch —
// 0, a quarter, a half — against the CPU's exact 0.6 of a pixel.
fn edge_cover(u: f32) -> f32 {
    let w = across(u);
    return clamp(0.5 + u / w, 0.0, 1.0) * clamp(0.5 + (1.0 - u) / w, 0.0, 1.0);
}

// A layer's surface coming back down in its colour overlay: each
// premultiplied channel `w` of the way to `tint` at the pixel's own
// cover, the CPU renderer's `wear_overlay`. Nothing at a strength of
// none, which is every picture that is not such a surface.
fn overlaid(p: vec4f, w: f32, tint: vec3f) -> vec4f {
    if w <= 0.0 {
        return p;
    }
    return vec4f(p.rgb + (tint * p.a - p.rgb) * w, p.a);
}

@fragment
fn fs_image(in: ImageOut) -> @location(0) vec4f {
    let border = edge_cover(in.uv.x) * edge_cover(in.uv.y);
    // `grad` is how much of the picture a pixel covers, in texels along
    // its own axes, and how many taps a side average it: one by one at
    // its own size or larger, a grid when it is shrunk — spread evenly
    // over the footprint, as the CPU renderer spreads them.
    let taps = vec2i(max(in.grad.zw, vec2f(1.0)));
    let size = vec2f(textureDimensions(image));
    // Drawn from a level of the picture's pyramid, how much of that
    // level the picture spans — under one where its last texel is only
    // part the picture's; nothing, read as all of it, otherwise.
    let span = select(vec2f(1.0), in.params.zw, in.params.zw > vec2f(0.0));
    let uv = in.uv * span;
    var sampled = vec4f(0.0);
    if taps.x > 1 || taps.y > 1 {
        for (var j = 0; j < taps.y; j = j + 1) {
            for (var i = 0; i < taps.x; i = i + 1) {
                let o = (vec2f(f32(i), f32(j)) + vec2f(0.5)) / vec2f(taps) - vec2f(0.5);
                sampled = sampled + textureSampleLevel(image, image_sampler, uv + o * in.grad.xy / size, 0.0);
            }
        }
        sampled = sampled / f32(taps.x * taps.y);
    } else {
        sampled = textureSampleLevel(image, image_sampler, clamp(uv, vec2f(0.0), span), 0.0);
    }
    return overlaid(sampled, in.params.y, in.extra) * in.alpha * border * mask_cover(in.page, in.mask);
}

// A picture with its corners moved: carried onto its quad by a
// projective map, read back here a pixel at a time, which is how the
// CPU renderer draws one (`draw_raster_warped`) and the only way to —
// a quad's two triangles interpolate straight lines, and a picture in
// perspective is not straight across either of them. The map *back*,
// from the device to the picture's own texels, rides the quad as nine
// numbers: `params` holds the first four and `grad` the next four,
// scaled so the ninth is `extra.z` and the denominator is one at the
// quad's middle, and so positive over all of it. `extra.x` is the
// picture's height and `extra.y` how many levels of its pyramid stand
// under it in the one texture; its width is the texture's.
fn back_at(p: vec4f, g: vec4f, b8: f32, x: f32, y: f32) -> vec2f {
    let den = g.z * x + g.w * y + b8;
    return vec2f(p.x * x + p.y * y + p.z, p.w * x + g.x * y + g.y) / den;
}

// The part of the pixel at (`px`, `py`) the picture's quad covers: the
// pixel's square clipped to each edge in turn and its area, which is
// the CPU renderer's `covered` and so the same exact fraction. The
// quad's edges are the lines the map sends the picture's own edges to
// — `u = 0`, `u = W`, `v = 0`, `v = H` — each of which, with the
// denominator positive over the quad, is one side of a line on the
// device whose numbers are read straight off the map.
fn warped_cover(p: vec4f, g: vec4f, b8: f32, W: f32, H: f32, px: f32, py: f32) -> f32 {
    var poly: array<vec2f, 8>;
    var n = 4u;
    poly[0] = vec2f(px, py);
    poly[1] = vec2f(px + 1.0, py);
    poly[2] = vec2f(px + 1.0, py + 1.0);
    poly[3] = vec2f(px, py + 1.0);
    for (var e = 0u; e < 4u; e = e + 1u) {
        var line: vec3f;
        switch e {
            case 0u: { line = vec3f(p.x, p.y, p.z); }
            case 1u: { line = vec3f(W * g.z - p.x, W * g.w - p.y, W * b8 - p.z); }
            case 2u: { line = vec3f(p.w, g.x, g.y); }
            default: { line = vec3f(H * g.z - p.w, H * g.w - g.x, H * b8 - g.y); }
        }
        var next: array<vec2f, 8>;
        var m = 0u;
        for (var j = 0u; j < n; j = j + 1u) {
            let a = poly[j];
            let b = poly[(j + 1u) % n];
            let sa = dot(line, vec3f(a, 1.0));
            let sb = dot(line, vec3f(b, 1.0));
            if sa >= 0.0 {
                next[m] = a;
                m = m + 1u;
            }
            if (sa >= 0.0) != (sb >= 0.0) {
                next[m] = a + (b - a) * (sa / (sa - sb));
                m = m + 1u;
            }
        }
        poly = next;
        n = m;
        if n == 0u {
            return 0.0;
        }
    }
    var area = 0.0;
    for (var j = 0u; j < n; j = j + 1u) {
        let a = poly[j];
        let b = poly[(j + 1u) % n];
        area = area + a.x * b.y - b.x * a.y;
    }
    return min(abs(area) * 0.5, 1.0);
}

// One bilinear tap at `s`, in the texels of a level `w` by `h` that
// starts `off` rows down the texture, clamped at the level's edge as
// the CPU's `warped_sample` clamps its.
fn warped_bilinear(s: vec2f, w: u32, h: u32, off: u32) -> vec4f {
    let last = vec2f(f32(w) - 1.0, f32(h) - 1.0);
    let u = s - vec2f(0.5);
    let u0 = floor(u);
    let fr = u - u0;
    let x0 = i32(clamp(u0.x, 0.0, last.x));
    let x1 = i32(clamp(u0.x + 1.0, 0.0, last.x));
    let y0 = i32(clamp(u0.y, 0.0, last.y)) + i32(off);
    let y1 = i32(clamp(u0.y + 1.0, 0.0, last.y)) + i32(off);
    let top = mix(textureLoad(image, vec2i(x0, y0), 0), textureLoad(image, vec2i(x1, y0), 0), fr.x);
    let bottom = mix(textureLoad(image, vec2i(x0, y1), 0), textureLoad(image, vec2i(x1, y1), 0), fr.x);
    return mix(top, bottom, fr.y);
}

// The picture read at `here` by a pixel whose footprint there is `foot`
// of its texels, from level `k` of it: one tap, or a grid of them over
// the footprint — a texel apart on the picture itself, two on a level,
// the CPU's `warped_sample` tap for tap. Level `k` is `k` halvings of
// the picture, each rounded up, stacked under it in the texture.
fn warped_level(here: vec2f, foot: vec2f, k: i32, W: f32, H: f32) -> vec4f {
    var w = u32(W);
    var h = u32(H);
    var off = 0u;
    for (var i = 0; i < k; i = i + 1) {
        off = off + h;
        w = (w + 1u) / 2u;
        h = (h + 1u) / 2u;
    }
    let step = f32(1u << u32(k));
    let apart = select(2.0, 1.0, k == 0);
    let l = here / step;
    let f = max(foot / step, vec2f(1.0));
    let taps = clamp(vec2i(ceil(f / apart)), vec2i(1), vec2i(8));
    if taps.x == 1 && taps.y == 1 {
        return warped_bilinear(l, w, h, off);
    }
    var acc = vec4f(0.0);
    for (var j = 0; j < taps.y; j = j + 1) {
        for (var i = 0; i < taps.x; i = i + 1) {
            let o = (vec2f(f32(i), f32(j)) + vec2f(0.5)) / vec2f(taps) - vec2f(0.5);
            acc = acc + warped_bilinear(l + o * f, w, h, off);
        }
    }
    return acc / f32(taps.x * taps.y);
}

@fragment
fn fs_warped(in: ImageOut) -> @location(0) vec4f {
    let p = in.params;
    let g = in.grad;
    let H = in.extra.x;
    let depth = i32(in.extra.y);
    let b8 = in.extra.z;
    let W = f32(textureDimensions(image).x);
    let px = floor(in.page);
    let cover = warped_cover(p, g, b8, W, H, px.x, px.y);
    if cover <= 0.0 {
        return vec4f(0.0);
    }
    // Where the pixel's centre lands in the picture, and how far a step
    // of one pixel goes there each way: the map's own slope, read off
    // its neighbours as the CPU reads it.
    let c = px + vec2f(0.5);
    let here = back_at(p, g, b8, c.x, c.y);
    let sx = back_at(p, g, b8, c.x + 1.0, c.y) - here;
    let sy = back_at(p, g, b8, c.x, c.y + 1.0) - here;
    let foot = max(vec2f(abs(sx.x) + abs(sy.x), abs(sx.y) + abs(sy.y)), vec2f(1.0));
    let least = min(foot.x, foot.y);
    var sampled: vec4f;
    // Shrunk at all, the pixel is between two levels and is read from
    // both, mixed by how far between them it is.
    if least > 1.0 && depth > 0 {
        let at = min(log2(least), f32(depth));
        let k = i32(floor(at));
        let between = clamp(at - f32(k), 0.0, 1.0);
        sampled = warped_level(here, foot, k, W, H);
        if between > 0.0 && k < depth {
            sampled = mix(sampled, warped_level(here, foot, k + 1, W, H), between);
        }
    } else {
        sampled = warped_level(here, foot, 0, W, H);
    }
    return sampled * in.alpha * cover * mask_cover(in.page, in.mask);
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
    // A block set in more than one colour carries them on the backdrop's
    // slot, which text never reads: the colour of the texel the pixel
    // falls in, not a mix of its neighbours, and the quad's colour is then
    // only the layer's opacity.
    var color = in.color;
    if in.grad.x > 0.5 {
        let at = clamp(vec2i(floor(in.uv)) + vec2i(1, 1), vec2i(0), vec2i(size) - vec2i(1));
        color = textureLoad(backdrop, at, 0) * in.color.a;
    }
    return color * select(0.0, cov, inked) * mask_cover(in.page, in.mask);
}

// Where a point of the shape's normalized box sits along its gradient:
// the projection onto the line from `from` to `to`, or the distance from
// the centre in units of the radius, clamped past either end — the same
// arithmetic the CPU renderer does per pixel.
fn ramp_at(uv: vec2f, geom: vec4f, kind: f32, spread: f32, focal_y: f32) -> f32 {
    // Conic: once round the centre in `geom.xy` from the angle in
    // `geom.z`, clockwise on the page — `chitrakar_doc::conic_turn`, the
    // centre itself at the start.
    if kind > 1.5 {
        let d = uv - geom.xy;
        if d.x == 0.0 && d.y == 0.0 {
            return 0.0;
        }
        let t = (degrees(atan2(d.y, d.x)) - geom.z) / 360.0;
        return t - floor(t);
    }
    if kind > 0.5 {
        // A ring of no size is the last colour throughout.
        if geom.z < 1e-6 {
            return 1.0;
        }
        // Rings about their centre measured as they always were.
        let f = vec2f(geom.w, focal_y);
        if all(f == vec2f(0.0)) {
            return spread_at(length(uv - geom.xy) / geom.z, spread);
        }
        return spread_at(focal_ramp((uv - geom.xy) / geom.z, f), spread);
    }
    let d = geom.zw - geom.xy;
    let len2 = dot(d, d);
    // A line of no length, the first.
    if len2 < 1e-12 {
        return 0.0;
    }
    return spread_at(dot(uv - geom.xy, d) / len2, spread);
}

// Which ring out from a radial's focus a point is on, both measured from
// the centre in radii — `chitrakar_doc::focal_ramp`, said the same way:
// the root of `k t² − 2 (w·f) t − |w|² = 0` that is not negative.
fn focal_ramp(p: vec2f, f: vec2f) -> f32 {
    let w = p - f;
    let ww = dot(w, w);
    let wf = dot(w, f);
    let k = 1.0 - dot(f, f);
    let root = sqrt(max(wf * wf + k * ww, 0.0));
    if wf >= 0.0 {
        return (wf + root) / k;
    }
    if root - wf > 1e-30 {
        return ww / (root - wf);
    }
    return 0.0;
}

// What a gradient does past the ends of its ramp — `Spread::place`:
// 0 pads with the end colours, 1 reflects the ramp back and forth, 2
// repeats it from its start.
fn spread_at(t: f32, spread: f32) -> f32 {
    if spread > 1.5 {
        return t - floor(t);
    }
    if spread > 0.5 {
        let u = t - 2.0 * floor(t / 2.0);
        return select(u, 2.0 - u, u > 1.0);
    }
    return clamp(t, 0.0, 1.0);
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
    return ramp_color(ramp_at(uv, in.grad, in.color.r, in.color.g, in.color.b))
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
    return ramp_color(ramp_at(in.uv, in.grad, in.color.r, in.color.g, in.color.b))
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
    // Held at nothing below and let run above: a display encoding stops
    // at white, but the light this engine carries does not, and holding
    // a channel at one here made every blend but Normal a highlight
    // crusher — `Lighten` of a half grey over a ground at 2.415 came
    // back 1.000. The curve is a power law and carries on happily.
    let s = max(c / a, vec3f(0.0));
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
    let dst = textureSampleLevel(backdrop, backdrop_sampler, in.uv, 0.0);
    // Past the last blend mode: a frame drawn where it stands, on a
    // surface that began as a copy of what is under it so that what it
    // holds could read the page, mixed back by its fade and its mask —
    // the CPU renderer's `lerp(under, drawn, a)`. Where the frame drew
    // nothing the surface is still the copy, and the mix is nothing.
    if in.mode >= 16.0 {
        let drawn = textureSampleLevel(image, image_sampler, in.uv, 0.0);
        return mix(dst, drawn, in.alpha * mask_cover(in.page, in.mask));
    }
    let src = overlaid(textureSampleLevel(image, image_sampler, in.uv, 0.0), in.params.y, in.extra)
        * in.alpha
        * mask_cover(in.page, in.mask);
    return composite(src, dst, i32(in.mode));
}

// `src` brought down onto `dst` by a blend mode, both premultiplied: the
// W3C's (1-da)*s + (1-sa)*d + sa*da*B.
fn composite(src: vec4f, dst: vec4f, mode: i32) -> vec4f {
    let sa = src.a;
    let da = dst.a;
    let b = max(blended(mode, shown3(src.rgb, sa), shown3(dst.rgb, da)), vec3f(0.0));
    let light = vec3f(to_light(b.r), to_light(b.g), to_light(b.b));
    return vec4f(
        (1.0 - da) * src.rgb + (1.0 - sa) * dst.rgb + sa * da * light,
        sa + da * (1.0 - sa),
    );
}

// A frame brought down by a blend of its own while holding what works on
// the page, drawn twice as the reference draws it: where it stands, on a
// surface that began as what is under it (`image`), and on a surface of
// its own, where what it paints is all there is (`parent_tex`). What it
// paints meets what is under it (`backdrop`) by the blend; what drawing
// it in place did beyond laying that paint — what its contents made of
// the page — is added as it was, the way a blend means nothing to an
// adjustment. Then the whole is mixed back by the frame's fade and mask.
@fragment
fn fs_twice(in: ImageOut) -> @location(0) vec4f {
    let p = vec2i(floor(in.uv * page.size));
    let under = textureLoad(backdrop, p, 0);
    let drawn = textureLoad(image, p, 0);
    let paint = textureLoad(parent_tex, p, 0);
    let laid = paint + under * (1.0 - paint.a);
    let met = composite(paint, under, i32(in.mode));
    let kept = vec4f(max(met.rgb + drawn.rgb - laid.rgb, vec3f(0.0)), drawn.a);
    return mix(under, kept, in.alpha * mask_cover(in.page, in.mask));
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
fn adjusted(kind: i32, p: vec4f, q: vec4f, e: vec3f, c: vec3f) -> vec3f {
    switch kind {
        // Exposure: stops, which is a gain.
        case 1: {
            return c * pow(2.0, p.y);
        }
        // Brightness and contrast, about the middle.
        case 2: {
            return max((c + vec3f(p.y) - vec3f(0.5)) * (1.0 + p.z) + vec3f(0.5), vec3f(0.0));
        }
        // Hue rotation (the feColorMatrix one), then saturation about
        // the pixel's own luminance, then a lightness offset.
        case 3: {
            // Colorize: the tint in `q.yz`, at the lightness each pixel
            // shows at, lifted in light by `p.w` as the plain one is.
            if q.x > 0.5 {
                let shown = vec3f(to_shown(clamp(c.r, 0.0, 1.0)), to_shown(clamp(c.g, 0.0, 1.0)), to_shown(clamp(c.b, 0.0, 1.0)));
                let tinted = from_hsl(vec3f(q.y, q.z, to_hsl(shown).z));
                return max(vec3f(to_light(tinted.r), to_light(tinted.g), to_light(tinted.b)) + vec3f(p.w), vec3f(0.0));
            }
            let a = radians(p.y);
            let sn = sin(a);
            let cs = cos(a);
            let m0 = vec3f(0.213 + cs * 0.787 - sn * 0.213, 0.715 - cs * 0.715 - sn * 0.715, 0.072 - cs * 0.072 + sn * 0.928);
            let m1 = vec3f(0.213 - cs * 0.213 + sn * 0.143, 0.715 + cs * 0.285 + sn * 0.140, 0.072 - cs * 0.072 - sn * 0.283);
            let m2 = vec3f(0.213 - cs * 0.213 - sn * 0.787, 0.715 - cs * 0.715 + sn * 0.715, 0.072 + cs * 0.928 + sn * 0.072);
            let turned = vec3f(dot(m0, c), dot(m1, c), dot(m2, c));
            let l = dot(vec3f(0.2126, 0.7152, 0.0722), turned);
            return max(vec3f(l) + (turned - vec3f(l)) * (1.0 + p.z) + vec3f(p.w), vec3f(0.0));
        }
        // Levels: an input range, a gamma, an output range.
        case 4: {
            let span = max(p.z - p.y, 1e-3);
            let e = 1.0 / max(p.w, 0.05);
            let v = pow(max((c - vec3f(p.y)) / span, vec3f(0.0)), vec3f(e));
            return max(vec3f(q.x) + v * (q.y - q.x), vec3f(0.0));
        }
        // White balance: a gain per channel, half the slider's travel at
        // each end so the extremes still hold a picture.
        case 5: {
            let warm = clamp(p.y, -1.0, 1.0) * 0.5;
            let mag = clamp(p.z, -1.0, 1.0) * 0.5;
            return max(c * vec3f(1.0 + warm, 1.0 - mag, 1.0 - warm), vec3f(0.0));
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
            return max(vec3f(l) + (c - vec3f(l)) * s, vec3f(0.0));
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
            return vec3f(max(dot(c, w), 0.0));
        }
        // Inverted on the values a device shows: linear 0.5 shows as 188
        // and would come back a near-black rather than itself.
        case 8: {
            let k = clamp(p.y, 0.0, 1.0);
            let inside = clamp(c, vec3f(0.0), vec3f(1.0));
            let over = max(c - inside, vec3f(0.0));
            let s = vec3f(to_shown(inside.r), to_shown(inside.g), to_shown(inside.b));
            let f = s + (vec3f(1.0) - s - s) * k;
            return vec3f(to_light(f.r), to_light(f.g), to_light(f.b)) + over * (1.0 - k);
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
        // Posterize: each channel held to `p.y` steps above black, even
        // as a device shows them. The steps are worked out on the CPU, so
        // both renderers hold a picture to the same number of them.
        case 20: {
            let inside = clamp(c, vec3f(0.0), vec3f(1.0));
            let s = vec3f(to_shown(inside.r), to_shown(inside.g), to_shown(inside.b));
            // floor of a half up, as the CPU rounds: WGSL rounds a half to even.
            let q = floor(s * p.y + vec3f(0.5)) / p.y;
            return vec3f(to_light(q.r), to_light(q.g), to_light(q.b));
        }
        // Threshold: white or black by the brightness the pixel shows at.
        case 21: {
            let l = clamp(dot(vec3f(0.2126, 0.7152, 0.0722), c), 0.0, 1.0);
            if to_shown(l) >= p.y {
                return vec3f(1.0);
            }
            return vec3f(0.0);
        }
        // Channel mixer: each channel made again from all three, in light,
        // by the rows the CPU folded and held (`channel_mix`). Nine
        // numbers, row by row across `p.yzw`, `q` and `e.xy`; one with a
        // constant to add is kind 25, which reads it off a texel.
        case 22: {
            let r = dot(vec3f(p.y, p.z, p.w), c);
            let g = dot(vec3f(q.x, q.y, q.z), c);
            let b = dot(vec3f(q.w, e.x, e.y), c);
            return max(vec3f(r, g, b), vec3f(0.0));
        }
        // Photo filter: the colour through a glass of `p.yzw`, mixed back
        // in by `q.x` on the values a device shows (what is above white
        // through it in light); with `q.y` set, the brightness it had,
        // back, by a scale.
        case 23: {
            let inside = clamp(c, vec3f(0.0), vec3f(1.0));
            let over = c - inside;
            let s = vec3f(to_shown(inside.r), to_shown(inside.g), to_shown(inside.b));
            let g = clamp(p.yzw, vec3f(0.0), vec3f(1.0));
            let gs = vec3f(to_shown(g.r), to_shown(g.g), to_shown(g.b));
            let m = s + (s * gs - s) * q.x;
            let through = vec3f(to_light(m.r), to_light(m.g), to_light(m.b))
                + over * (vec3f(1.0) + (p.yzw - vec3f(1.0)) * q.x);
            if q.y > 0.5 {
                let luma = vec3f(0.2126, 0.7152, 0.0722);
                let now = dot(luma, through);
                if now > 1e-6 {
                    return through * (dot(luma, c) / now);
                }
            }
            return through;
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
            // A curve is drawn over the display encoding, which stops at
            // white, so a channel above it is off the end of the graph.
            // Reading the table holds it at the last point, which turns
            // every highlight above one into white and a curve down the
            // diagonal into a highlight crusher. What is above white
            // takes the curve's gain at white instead, as the reference
            // renderer does: the diagonal has a gain of one and is the
            // identity, and the two meet exactly at one.
            let inside = clamp(c, vec3f(0.0), vec3f(1.0));
            let over = max(c - inside, vec3f(0.0));
            let shown = vec3f(to_shown(inside.r), to_shown(inside.g), to_shown(inside.b));
            let master = vec3f(table_at(shown.r).r, table_at(shown.g).r, table_at(shown.b).r);
            let own = vec3f(table_at(master.r).g, table_at(master.g).b, table_at(master.b).a);
            let white_m = table_at(1.0).r;
            let white = vec3f(
                to_light(table_at(white_m).g),
                to_light(table_at(white_m).b),
                to_light(table_at(white_m).a),
            );
            return vec3f(to_light(own.r), to_light(own.g), to_light(own.b)) + over * white;
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
        // A channel mixer with a constant to add: the nine numbers where
        // kind 22 has them, and the constant on the one texel bound
        // where a table would be.
        case 25: {
            let k = textureLoad(image, vec2i(0, 0), 0).rgb;
            let r = dot(vec3f(p.y, p.z, p.w), c) + k.r;
            let g = dot(vec3f(q.x, q.y, q.z), c) + k.g;
            let b = dot(vec3f(q.w, e.x, e.y), c) + k.b;
            return max(vec3f(r, g, b), vec3f(0.0));
        }
        // A colour lookup: a 3D table of `p.y` points a side, its blue
        // slices laid `p.z` to a row of tiles, read on the values a device
        // shows between its eight nearest entries — red mixed first, then
        // green, then blue, the CPU renderer's own order.
        case 24: {
            let n = i32(p.y);
            let k = i32(p.z);
            let shown = vec3f(to_shown(clamp(c.r, 0.0, 1.0)), to_shown(clamp(c.g, 0.0, 1.0)), to_shown(clamp(c.b, 0.0, 1.0)));
            let x = shown * f32(n - 1);
            let i = min(vec3i(floor(x)), vec3i(n - 2));
            let f = x - vec3f(i);
            let c00 = mix_line(lut_at(i.x, i.y, i.z, n, k), lut_at(i.x + 1, i.y, i.z, n, k), f.x);
            let c10 = mix_line(lut_at(i.x, i.y + 1, i.z, n, k), lut_at(i.x + 1, i.y + 1, i.z, n, k), f.x);
            let c01 = mix_line(lut_at(i.x, i.y, i.z + 1, n, k), lut_at(i.x + 1, i.y, i.z + 1, n, k), f.x);
            let c11 = mix_line(lut_at(i.x, i.y + 1, i.z + 1, n, k), lut_at(i.x + 1, i.y + 1, i.z + 1, n, k), f.x);
            let out = mix_line(mix_line(c00, c10, f.y), mix_line(c01, c11, f.y), f.z);
            return vec3f(to_light(out.r), to_light(out.g), to_light(out.b));
        }
        default: {
            return c;
        }
    }
}

// One entry of a colour lookup's table: slice `b` is tile `b % k` across
// and `b / k` down.
fn lut_at(r: i32, g: i32, b: i32, n: i32, k: i32) -> vec3f {
    return textureLoad(image, vec2i((b % k) * n + r, (b / k) * n + g), 0).rgb;
}

// The CPU renderer's `ripple_sine`: the sine of the share of a
// wavelength past the last whole one.
fn ripple_sine(at: f32, length: f32) -> f32 {
    let t = at / length;
    return sin((t - floor(t)) * 6.28318530717958647);
}

// A pixel of what is under the layer, the edge read again past the edge
// — the page's edge, or a frame's for a filter inside one, which is where
// the CPU renderer's own reading stops (it is handed that rectangle to
// work in), rather than the surface's. Within the texture either way.
fn texel_at(i: vec2i, size: vec2i) -> vec4f {
    let lo = max(vec2i(page.lo), vec2i(0));
    let hi = min(vec2i(page.hi), size) - vec2i(1);
    return textureLoad(backdrop, clamp(i, lo, hi), 0);
}

// The CPU renderer's lerp over a whole pixel, alpha and all.
fn mix_px(a: vec4f, b: vec4f, t: f32) -> vec4f {
    return a + (b - a) * t;
}

// `a + (b - a)·t`, the CPU renderer's own lerp, rather than `mix`, which
// rounds the other way round.
fn mix_line(a: vec3f, b: vec3f, t: f32) -> vec3f {
    return a + (b - a) * t;
}

@fragment
fn fs_adjust(in: ImageOut) -> @location(0) vec4f {
    let was = textureSampleLevel(backdrop, backdrop_sampler, in.uv, 0.0);
    let weight = in.alpha * mask_cover(in.page, in.mask);
    let kind = i32(in.mode);
    // A warp moves the picture, so it can bring it onto a pixel where
    // nothing was; everything else leaves an uncovered pixel as it is.
    let warps = kind == 19 || kind == 26 || kind == 27 || kind == 30;
    if weight <= 0.0 || (was.a <= 0.0 && !warps) {
        return was;
    }
    // Ten to thirteen read a table, fourteen to nineteen are the filters
    // of one pixel, and the rest are numbers again — the adjustments that
    // came after the filters had taken the next numbers, and twenty-six
    // to thirty, the pinch, the ripple, find edges, oil paint and lens
    // correction, which came after those.
    if (kind >= 14 && kind < 20) || (kind >= 26 && kind < 31) {
        return filtered(kind, in.params, in.grad, in.extra, in.page, was, weight);
    }
    // Straight alpha in, premultiplied out, which is where the
    // adjustments are stated.
    let straight = was.rgb / was.a;
    var out = adjusted(kind, in.params, in.grad, in.extra, straight);
    if (kind >= 10 && kind < 14) || kind == 24 || kind == 25 {
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
fn filtered(kind: i32, p: vec4f, g: vec4f, e: vec3f, at: vec2f, was: vec4f, weight: f32) -> vec4f {
    // Where this pixel is in the space the layer lives in. `g` is the
    // inverse of the view's linear part and `e.xy` is the view's own
    // translation, laid out and read exactly as `Inverse::of` and
    // `Inverse::at` lay out and read theirs — a grain cell is a `floor`,
    // and a last bit rounding the other way puts a speck in the next
    // cell. While the view is the identity this is the page pixel; under
    // a view, or inside a copy that draws what it copies somewhere else,
    // it is not, and a filter anchored to the surface stops being
    // anchored to the picture.
    let rel = at - vec2f(e.x, e.y);
    let here = vec2f(g.x * rel.x + g.z * rel.y, g.y * rel.x + g.w * rel.y);
    if kind == 17 {
        // A halftone: the CPU renderer's `halftone_ink`, step for step.
        // Into the turned grid, to the middle of this cell, back out.
        let cell = p.y;
        let s = sin(p.z);
        let c = cos(p.z);
        let q = vec2f(c * here.x + s * here.y, -s * here.x + c * here.y);
        let mid = (floor(q / cell) + vec2f(0.5)) * cell;
        let centre = vec2f(c * mid.x - s * mid.y, s * mid.x + c * mid.y);
        // Back onto the surface through the inverse's own inverse, the
        // way the CPU renderer works it, to the pixel the middle falls
        // on — a quarter in, so a middle on an edge is never a toss.
        let det = g.x * g.w - g.y * g.z;
        let fwd = vec4f(g.w / det, -g.y / det, -g.z / det, g.x / det);
        let dev = vec2f(fwd.x * centre.x + fwd.z * centre.y + e.x, fwd.y * centre.x + fwd.w * centre.y + e.y);
        let size = vec2i(textureDimensions(backdrop));
        let under = texel_at(vec2i(floor(dev + vec2f(0.25))), size);
        var lum = 1.0;
        if under.a > 0.0 {
            lum = dot(vec3f(0.2126, 0.7152, 0.0722), under.rgb) / under.a;
        }
        let dark = 1.0 - to_shown(clamp(lum, 0.0, 1.0));
        var radius = cell * sqrt(dark / 3.14159265);
        if dark > 0.78539816 {
            radius = cell * (0.5 + (dark - 0.78539816) / (1.0 - 0.78539816) * (0.70710678 - 0.5));
        }
        let sharp = 1.0 / max(sqrt(abs(det)), 1e-6);
        let small = clamp(2.0 * radius * sharp, 0.0, 1.0);
        let ink = clamp((radius - distance(here, centre)) * sharp + 0.5, 0.0, 1.0) * small;
        let paper = (1.0 - ink) * was.a;
        return mix(was, vec4f(paper, paper, paper, was.a), weight);
    }
    if kind == 19 || kind == 26 || kind == 27 || kind == 30 {
        // A twirl, a pinch or a ripple: the CPU renderer's
        // `twirl_source`, `pinch_source` or `ripple_source`, then its
        // `warp`, step for step.
        let mid = vec2f(p.w, e.z);
        let reach = p.z;
        let d = here - mid;
        let r = sqrt(d.x * d.x + d.y * d.y);
        var src = here;
        if kind == 27 {
            src = vec2f(here.x + p.y * ripple_sine(here.y, p.z), here.y + p.y * ripple_sine(here.x, p.z));
        } else if kind == 30 {
            // A lens correction: `lens_source`, the corner `reach` away.
            let rho2 = (d.x * d.x + d.y * d.y) / (reach * reach);
            let s = 1.0 - p.y * rho2;
            src = vec2f(mid.x + d.x * s, mid.y + d.y * s);
        } else if r < reach {
            if kind == 19 {
                let left = 1.0 - r / reach;
                let t = -p.y * left * left;
                let s = sin(t);
                let c = cos(t);
                src = vec2f(mid.x + c * d.x - s * d.y, mid.y + s * d.x + c * d.y);
            } else if r > 0.0 {
                let k = pow(r / reach, p.y) * reach / r;
                src = vec2f(mid.x + d.x * k, mid.y + d.y * k);
            }
        }
        let det = g.x * g.w - g.y * g.z;
        let fwd = vec4f(g.w / det, -g.y / det, -g.z / det, g.x / det);
        let dev = vec2f(fwd.x * src.x + fwd.z * src.y + e.x, fwd.y * src.x + fwd.w * src.y + e.y);
        let f = clamp(dev - vec2f(0.5), vec2f(-1e6), vec2f(1e6));
        let base = floor(f);
        let t = f - base;
        let i = vec2i(base);
        let size = vec2i(textureDimensions(backdrop));
        let top = mix_px(texel_at(i, size), texel_at(i + vec2i(1, 0), size), t.x);
        let bottom = mix_px(texel_at(i + vec2i(0, 1), size), texel_at(i + vec2i(1, 1), size), t.x);
        return mix_px(was, mix_px(top, bottom, t.y), weight);
    }
    if kind == 29 {
        // Oil paint: the CPU renderer's `oil_painted`. The window walked
        // once, each pixel counted into every corner square it is in, then
        // the smoothest square's average — by the same clear margin.
        let size = vec2i(textureDimensions(backdrop));
        let px = vec2i(floor(at));
        let r = i32(p.y);
        var sums: array<vec4f, 4>;
        var light: array<vec4f, 4>;
        for (var dy = -r; dy <= r; dy = dy + 1) {
            for (var dx = -r; dx <= r; dx = dx + 1) {
                let q = texel_at(px + vec2i(dx, dy), size);
                let l = dot(vec3f(0.2126, 0.7152, 0.0722), q.rgb);
                let two = vec4f(l, l * l, q.a, q.a * q.a);
                if dx <= 0 && dy <= 0 {
                    sums[0] = sums[0] + q;
                    light[0] = light[0] + two;
                }
                if dx >= 0 && dy <= 0 {
                    sums[1] = sums[1] + q;
                    light[1] = light[1] + two;
                }
                if dx <= 0 && dy >= 0 {
                    sums[2] = sums[2] + q;
                    light[2] = light[2] + two;
                }
                if dx >= 0 && dy >= 0 {
                    sums[3] = sums[3] + q;
                    light[3] = light[3] + two;
                }
            }
        }
        let count = f32((r + 1) * (r + 1));
        var best = 3.4e38;
        var mean = was;
        for (var k = 0; k < 4; k = k + 1) {
            // Lit unevenly or covered unevenly: black and nothing are
            // equally dark.
            let m = light[k].x / count;
            let c = light[k].z / count;
            let v = (light[k].y / count - m * m) + (light[k].w / count - c * c);
            if v < best - 1e-4 {
                best = v;
                mean = sums[k] / count;
            }
        }
        return mix_px(was, mean, weight);
    }
    if kind == 28 {
        // Find edges: the CPU renderer's `edge_white`, a channel at a
        // time, over the eight pixels a step round this one.
        let size = vec2i(textureDimensions(backdrop));
        let px = vec2i(floor(at));
        let n = i32(p.y);
        let a = texel_at(px + vec2i(-n, -n), size);
        let b = texel_at(px + vec2i(0, -n), size);
        let c = texel_at(px + vec2i(n, -n), size);
        let d = texel_at(px + vec2i(-n, 0), size);
        let f = texel_at(px + vec2i(n, 0), size);
        let gg = texel_at(px + vec2i(-n, n), size);
        let h = texel_at(px + vec2i(0, n), size);
        let i = texel_at(px + vec2i(n, n), size);
        let gx = (c.rgb + 2.0 * f.rgb + i.rgb) - (a.rgb + 2.0 * d.rgb + gg.rgb);
        let gy = (gg.rgb + 2.0 * h.rgb + i.rgb) - (a.rgb + 2.0 * b.rgb + c.rgb);
        let steep = sqrt(gx * gx + gy * gy) / 4.0;
        let white = clamp(vec3f(was.a) - p.z * steep, vec3f(0.0), vec3f(was.a));
        return mix_px(was, vec4f(white, was.a), weight);
    }
    if kind == 18 {
        // An emboss: the CPU renderer's own, with the offset it rounded.
        // How much lighter the picture is the offset toward the light
        // than as far from it, the edge read again past the edge.
        let size = vec2i(textureDimensions(backdrop));
        let px = vec2i(floor(at));
        let step = vec2i(i32(p.y), i32(p.z));
        let toward = texel_at(px + step, size);
        let away = texel_at(px - step, size);
        let luma = vec3f(0.2126, 0.7152, 0.0722);
        let rise = dot(luma, toward.rgb) - dot(luma, away.rgb);
        let g = clamp(0.5 * was.a - 0.5 * p.w * rise, 0.0, was.a);
        return mix(was, vec4f(g, g, g, was.a), weight);
    }
    if kind == 14 {
        // Measured from the page's own middle in the page's own units,
        // so panning slides the picture under the darkening rather than
        // carrying the darkening along with it.
        let mid = page.doc * 0.5;
        let far = max(length(mid), 1e-3);
        let inner = clamp(p.z, 0.0, 0.999);
        let ease = clamp(p.w, 0.0, 1.0);
        let d = length(here - mid) / far;
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
    let cell = vec2i(floor(here / max(p.z, 1e-3)));
    let seed = u32(e.z) + u32(p.w) * 65536u;
    let one = (speck(cell, seed) - 0.5) * w;
    // Every channel moved together is film grain; each moved on its own
    // is a sensor's noise. Which of the two is the kind rather than a
    // flag on it: the four numbers a flag would have ridden are spoken
    // for by the inverse above.
    var shift = vec3f(one, one, one);
    if kind == 16 {
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
    if in.params.w > 2.5 {
        return median(in);
    }
    if in.params.w > 0.5 {
        return radial(in);
    }
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

// A pixel of what is under the layer, read between texels: the four
// round the point, each held to the page, the way the CPU renderer reads
// its snapshot of the page's rectangle. Pixel centres sit at a half.
fn held(p: vec2f) -> vec4f {
    return textureLoad(image, vec2i(clamp(p, page.lo, page.hi - vec2f(1.0, 1.0))), 0);
}

fn between(at: vec2f) -> vec4f {
    let u = at - vec2f(0.5, 0.5);
    let f = floor(u);
    let t = u - f;
    let a = held(f);
    let b = held(f + vec2f(1.0, 0.0));
    let c = held(f + vec2f(0.0, 1.0));
    let d = held(f + vec2f(1.0, 1.0));
    let top = a + (b - a) * t.x;
    let bottom = c + (d - c) * t.x;
    return top + (bottom - top) * t.y;
}

// Which of the display encoding's 256 steps each channel of a pixel is
// on, the way the CPU renderer bins it: colour by what it shows at,
// rounded, with above white held at white; alpha as it is.
fn steps_of(v: vec4f) -> vec4f {
    let c = clamp(v, vec4f(0.0), vec4f(1.0));
    return floor(vec4f(to_shown(c.r), to_shown(c.g), to_shown(c.b), c.a) * 255.0 + vec4f(0.5));
}

// A median: each channel the lower middle of the window `params.x`
// pixels each way, the page's edge carried on past it. The middle is
// found among the 256 steps by halving: count how much of the window is
// at or below the middle of what is left, and keep the half the median
// must be in — eight times, all four channels at once. What comes back
// is the step's own light, a colour held to the cover the pixel has, as
// the CPU renderer gives it.
fn median(in: ImageOut) -> vec4f {
    let r = i32(in.params.x);
    let here = floor(in.uv * page.size);
    let side = 2 * r + 1;
    let want = f32((side * side - 1) / 2);
    var lo = vec4f(0.0);
    var hi = vec4f(255.0);
    for (var halving = 0; halving < 8; halving = halving + 1) {
        let mid = floor((lo + hi) * 0.5);
        var count = vec4f(0.0);
        for (var dy = -r; dy <= r; dy = dy + 1) {
            for (var dx = -r; dx <= r; dx = dx + 1) {
                let b = steps_of(held(here + vec2f(f32(dx), f32(dy))));
                count = count + select(vec4f(0.0), vec4f(1.0), b <= mid);
            }
        }
        let over = count > vec4f(want);
        hi = select(hi, mid, over);
        lo = select(mid + vec4f(1.0), lo, over);
    }
    let a = lo.w / 255.0;
    let colour = vec3f(to_light(lo.x / 255.0), to_light(lo.y / 255.0), to_light(lo.z / 255.0));
    return vec4f(min(colour, vec3f(a)), a);
}

// A radial blur: each pixel the average of samples along its line out
// from the centre (a zoom) or along its circle round it (a spin), the
// line drawn in the space the filter lives in and brought to the page by
// the view's linear part — the CPU renderer's samples, its count and its
// spacing, so the two come out at the same picture.
//
// `params` is how far the line reaches, that times the view's scale, the
// centre across the surface and which of the two it is; `grad` is the
// linear part (a, b, c, d); `extra.x` is the centre down the surface.
fn radial(in: ImageOut) -> vec4f {
    let reach = in.params.x;
    let spin = in.params.w > 1.5;
    let centre = vec2f(in.params.z, in.extra.x);
    let l = in.grad;
    let det = l.x * l.w - l.y * l.z;
    let pixel = floor(in.uv * page.size);
    let d = pixel + vec2f(0.5, 0.5) - centre;
    // Back into the filter's own space, where the line is drawn.
    let v = vec2f(l.w * d.x - l.z * d.y, l.x * d.y - l.y * d.x) / det;
    let length = sqrt(v.x * v.x + v.y * v.y) * in.params.y;
    if length < 0.5 {
        return held(pixel);
    }
    // Odd, so that the pixel itself is one of the samples, and held to
    // the CPU renderer's 129.
    let n = min(i32(ceil(min(length, 256.0))) + 1, 129) | 1;
    let half = f32(n / 2);
    var sum = vec4f(0.0, 0.0, 0.0, 0.0);
    for (var k = 0; k < n; k = k + 1) {
        let t = (f32(k) - half) / f32(n - 1);
        var q = v * (1.0 + reach * t);
        if spin {
            let s = sin(reach * t);
            let c = cos(reach * t);
            q = vec2f(v.x * c - v.y * s, v.x * s + v.y * c);
        }
        sum = sum + between(centre + vec2f(l.x * q.x + l.z * q.y, l.y * q.x + l.w * q.y));
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
    // `brush_edge` in the reference renderer: the fade the softness asks
    // for inside the radius, and the pixel's antialiasing centred on its
    // ends rather than tucked inside them.
    let w = in.grad.w;
    let c = clamp((r + 0.5 * w - seg_distance(in.local, a, b)) / (r * in.grad.z + w), 0.0, 1.0);
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
// `params.yz` is the offset in device pixels, `params.x` the blend mode,
// and `params.w` one for a stroke that heals — whose coverage then comes
// with the shift it moves what it lifts by, in the three channels beside
// it (`fs_heal_spread`).
@fragment
fn fs_clone(in: ImageOut) -> @location(0) vec4f {
    let taken = textureSampleLevel(image, image_sampler, in.uv, 0.0);
    let cover = taken.a
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
    // Off the page there is nothing to clone: the page, not the surface,
    // which reaches past it under a view or when the scene is drawn with
    // room (`with_room`), and holds past it what layers crossing the edge
    // drew — which the CPU renderer never lays there.
    if at.x < page.lo.x || at.y < page.lo.y || at.x >= page.hi.x || at.y >= page.hi.y {
        return dst;
    }
    var lifted = textureLoad(backdrop, vec2i(at), 0);
    if lifted.a <= 0.0 {
        return dst;
    }
    if in.params.w > 0.5 {
        let a = lifted.a;
        lifted = vec4f(max(lifted.rgb / a + taken.rgb, vec3f(0.0)) * a, a);
    }
    let src = lifted * cover;
    let sa = src.a;
    let da = dst.a;
    let b = max(blended(i32(in.mode), shown3(src.rgb, sa), shown3(dst.rgb, da)), vec3f(0.0));
    let light = vec3f(to_light(b.r), to_light(b.g), to_light(b.b));
    return vec4f(
        (1.0 - da) * src.rgb + (1.0 - sa) * dst.rgb + sa * da * light,
        sa + da * (1.0 - sa),
    );
}

// A clone layer wearing an effect is drawn onto a surface of its own, so
// that the effect has a silhouette to grow from: what its strokes lay.
// But a clone paints with what is under it, and what is under it is not
// on that surface — it is the surface the layer is going to come down
// onto, which is still whatever it was when this one was opened, since
// everything drawn meanwhile went here. So both are read: the parent at
// `parent_tex`, and what the strokes have laid so far off the copy of
// this surface. What a stroke lifts is the one seen through the other —
// the page with the strokes before it already on it, which is what the
// reference renderer lifts from.
@group(3) @binding(2) var parent_tex: texture_2d<f32>;

fn seen(p: vec2i) -> vec4f {
    let laid = textureLoad(backdrop, p, 0);
    return laid + textureLoad(parent_tex, p, 0) * (1.0 - laid.a);
}

// `fs_clone` onto the layer's own surface. It lays what it lifts over
// what the strokes before it laid, and nothing else: its blend is the
// layer's, and comes when the surface does.
@fragment
fn fs_clone_aside(in: ImageOut) -> @location(0) vec4f {
    let taken = textureSampleLevel(image, image_sampler, in.uv, 0.0);
    let cover = taken.a * in.alpha * mask_cover(in.page, in.mask);
    let dst = textureSampleLevel(backdrop, backdrop_sampler, in.uv, 0.0);
    if cover <= 0.0 {
        return dst;
    }
    let at = floor(in.uv * page.size + vec2f(in.params.y, in.params.z));
    if at.x < page.lo.x || at.y < page.lo.y || at.x >= page.hi.x || at.y >= page.hi.y {
        return dst;
    }
    var lifted = seen(vec2i(at));
    if lifted.a <= 0.0 {
        return dst;
    }
    if in.params.w > 0.5 {
        let a = lifted.a;
        lifted = vec4f(max(lifted.rgb / a + taken.rgb, vec3f(0.0)) * a, a);
    }
    let src = lifted * cover;
    return src + dst * (1.0 - src.a);
}

// A healing stroke's two sums, a texel at a time: what it lifts and what
// is under it, each premultiplied and weighed by the stroke's coverage.
// Kept as colour and alpha together, so that the average each makes is
// its colour over its alpha — a pixel with next to no alpha is next to
// none of it, where an average of straight colours would let one decide
// as much as a solid pixel does, with whatever its last few bits say.
struct Sums {
    @location(0) lifted: vec4f,
    @location(1) under: vec4f,
};

// Each pixel's share, drawn onto a pair of textures the size of the
// rectangle the averages are taken over: `params.xy` is where that
// starts on the surface and `params.zw` the offset the stroke reads at,
// `grad` the box of the stroke's own region on the mask slot.
//
// A pixel counts where the stroke covers it, by how much there is of
// both what it lifts and what it lands on — the reference renderer's
// rule, read the way `fs_clone` reads, so the two agree on which pixel
// is lifted.
@fragment
fn fs_heal_terms(in: ImageOut) -> Sums {
    var out: Sums;
    out.lifted = vec4f(0.0);
    out.under = vec4f(0.0);
    let at = vec2i(floor(in.pos.xy)) + vec2i(in.params.xy);
    let c = textureLoad(image, at, 0).a * mask_cover(vec2f(at) + 0.5, in.grad);
    if c <= 0.0 {
        return out;
    }
    let source = floor(vec2f(at) + 0.5 + in.params.zw);
    if source.x < page.lo.x || source.y < page.lo.y || source.x >= page.hi.x || source.y >= page.hi.y {
        return out;
    }
    let lifted = textureLoad(backdrop, vec2i(source), 0);
    let under = textureLoad(backdrop, at, 0);
    // Each side weighed by the other's alpha, as the reference renderer
    // weighs them: a side with next to no alpha is next to no part of
    // either average, rather than all or nothing of it.
    out.lifted = lifted * (c * under.a);
    out.under = under * (c * lifted.a);
    return out;
}

// The same, for a heal on a clone layer's own surface: both of what it
// averages are the page as the strokes before it left it.
@fragment
fn fs_heal_terms_aside(in: ImageOut) -> Sums {
    var out: Sums;
    out.lifted = vec4f(0.0);
    out.under = vec4f(0.0);
    let at = vec2i(floor(in.pos.xy)) + vec2i(in.params.xy);
    let c = textureLoad(image, at, 0).a * mask_cover(vec2f(at) + 0.5, in.grad);
    if c <= 0.0 {
        return out;
    }
    let source = floor(vec2f(at) + 0.5 + in.params.zw);
    if source.x < page.lo.x || source.y < page.lo.y || source.x >= page.hi.x || source.y >= page.hi.y {
        return out;
    }
    let lifted = seen(vec2i(source));
    let under = seen(at);
    // Each side weighed by the other's alpha, as the reference renderer
    // weighs them: a side with next to no alpha is next to no part of
    // either average, rather than all or nothing of it.
    out.lifted = lifted * (c * under.a);
    out.under = under * (c * lifted.a);
    return out;
}

// Sixteen by sixteen texels of the pair before into one of the pair
// after. Past the edge of what is being added up is nothing, rather than
// whatever a load out of bounds would return.
@fragment
fn fs_heal_sum(in: ImageOut) -> Sums {
    let size = vec2i(textureDimensions(image));
    let base = vec2i(floor(in.pos.xy)) * 16;
    var out: Sums;
    out.lifted = vec4f(0.0);
    out.under = vec4f(0.0);
    for (var j = 0; j < 16; j++) {
        for (var i = 0; i < 16; i++) {
            let at = base + vec2i(i, j);
            if at.x < size.x && at.y < size.y {
                out.lifted += textureLoad(image, at, 0);
                out.under += textureLoad(mask_tex, at, 0);
            }
        }
    }
    return out;
}

// The shift the sums say, written beside the stroke's coverage: the
// average of what is under it less the average of what it lifts, which
// is nothing where either average has nothing in it.
@fragment
fn fs_heal_spread(in: ImageOut) -> @location(0) vec4f {
    let cover = textureSampleLevel(image, image_sampler, in.uv, 0.0).a;
    let lifted = textureLoad(mask_tex, vec2i(0), 0);
    let under = textureLoad(backdrop, vec2i(0), 0);
    var shift = vec3f(0.0);
    if lifted.a > 0.0 && under.a > 0.0 {
        shift = under.rgb / under.a - lifted.rgb / lifted.a;
    }
    return vec4f(shift, cover);
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
    // A clone's field stops at the page (`params.w`): the page's first
    // pixel in `extra.xy`, the first past it across in `extra.z` and down
    // in `params.z`. The reference grows a clone's effects cut to the
    // page, and past the window a field was built over there is nothing.
    if in.params.w == 1.0 {
        let p = floor(in.uv * page.size);
        if p.x < in.extra.x || p.y < in.extra.y || p.x >= in.extra.z || p.y >= in.params.z {
            return vec4f(0.0);
        }
    }
    // An outline is a distance from an edge, and an edge is where the
    // silhouette is half covered — so what a band is measured out from
    // is a yes or a no rather than a coverage, and `params.y` says at
    // what coverage the answer turns. It comes down as one, not as the
    // tint: what the band's own passes carry is a distance, and a
    // distance has no colour until it has been cut to a width.
    //
    // Or the middle of a line thinner than a pixel, which is never half
    // covered anywhere: the CPU renderer's `ridge`, read off the same
    // pixels with the same share of the composite taken, the same least
    // coverage (0.4 of the half that is an edge, which is the CPU's 0.2
    // of the layer's opacity) and the same margin (0.08 of that).
    if in.params.x == 2.0 {
        let size = vec2i(textureDimensions(image));
        let here = vec2i(floor(in.uv * vec2f(size)));
        let c = field_cover(here, size, in.alpha);
        let faint = 0.4 * in.params.y;
        let thin = c > faint
            && ridge(
                c,
                field_cover(here - vec2i(1, 0), size, in.alpha),
                field_cover(here + vec2i(1, 0), size, in.alpha),
                field_cover(here - vec2i(0, 1), size, in.alpha),
                field_cover(here + vec2i(0, 1), size, in.alpha),
                0.08 * faint,
            );
        return vec4f(select(0.0, 1.0, a >= in.params.y || thin));
    }
    let cover = select(a, 1.0 - a, in.params.x != 0.0);
    return in.grad * cover;
}

// How covered the layer is at `p`, with the composite's own share taken,
// and nothing past its edge.
fn field_cover(p: vec2i, size: vec2i, alpha: f32) -> f32 {
    if p.x < 0 || p.y < 0 || p.x >= size.x || p.y >= size.y {
        return 0.0;
    }
    return textureLoad(image, p, 0).a * alpha;
}

// The middle of a line thinner than a pixel: at least as covered as both
// neighbours along one axis, and more than one of them. A soft edge
// fading away is a slope and never this.
// Both by a margin, `step`, so that arithmetic's noise along a soft
// edge — which is not the same noise here as on the CPU — makes no ridge.
fn ridge(c: f32, l: f32, r: f32, u: f32, d: f32, step: f32) -> bool {
    let across_x = c >= max(l, r) - step && (c > l + step || c > r + step);
    let across_y = c >= max(u, d) - step && (c > u + step || c > d + step);
    return across_x || across_y;
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

// A shadow's spread, taken on the field as it was read at its offset —
// the CPU renderer's `Gain`. `gain.x` is the gain on the coverage, one
// for none; `gain.y` the tint's alpha, which the coverage is the field's
// alpha over. Held at full: the tint is the most it can come to.
fn gained(px: vec4f, gain: vec2f) -> vec4f {
    if gain.x <= 1.0 || gain.y <= 0.0 || px.a <= 0.0 {
        return px;
    }
    return px * min(gain.x, gain.y / px.a);
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
// A bevel's half, lit from its heights: the CPU renderer's
// `bevel_field`, a pixel at a time. The field is the silhouette blurred
// into heights; the slope is read off the eight around the pixel with
// SVG's quarter Sobel, and the highlight (`extra.z` one) or the shadow
// (minus one) takes the share by which the slope faces the light more,
// or less, than the flat top does. `params.xy` is toward the light
// across the page, `extra.x` how high a fully covered pixel stands,
// `grad` this half's colour; past the surface there is no height, as
// past the CPU's window there is none.
fn bevel_lit(in: ImageOut) -> vec4f {
    let size = vec2i(page.size);
    let p = vec2i(floor(in.uv * page.size));
    let h00 = field_tap(p + vec2i(-1, -1), size).a;
    let h10 = field_tap(p + vec2i(0, -1), size).a;
    let h20 = field_tap(p + vec2i(1, -1), size).a;
    let h01 = field_tap(p + vec2i(-1, 0), size).a;
    let h21 = field_tap(p + vec2i(1, 0), size).a;
    let h02 = field_tap(p + vec2i(-1, 1), size).a;
    let h12 = field_tap(p + vec2i(0, 1), size).a;
    let h22 = field_tap(p + vec2i(1, 1), size).a;
    let gx = (h20 + 2.0 * h21 + h22) - (h00 + 2.0 * h01 + h02);
    let gy = (h02 + 2.0 * h12 + h22) - (h00 + 2.0 * h10 + h20);
    let n = vec2f(-in.extra.x * 0.25 * gx, -in.extra.x * 0.25 * gy);
    let up = sqrt(max(0.0, 1.0 - dot(in.params.xy, in.params.xy)));
    let lit = (n.x * in.params.x + n.y * in.params.y + up) / sqrt(dot(n, n) + 1.0);
    let flat = clamp(up, 1e-4, 1.0 - 1e-4);
    var share = clamp((flat - lit) / flat, 0.0, 1.0);
    if in.extra.z > 0.0 {
        share = clamp((lit - flat) / (1.0 - flat), 0.0, 1.0);
    }
    return in.grad * share * textureSampleLevel(backdrop, backdrop_sampler, in.uv, 0.0).a * in.params.w;
}

@fragment
fn fs_effect(in: ImageOut) -> @location(0) vec4f {
    if in.extra.z != 0.0 {
        return bevel_lit(in) * in.alpha * mask_cover(in.page, in.mask);
    }
    var out = gained(field_at(in.uv, vec2f(in.params.x, in.params.y)), in.extra.xy);
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
    // A bevel's half is lit here on the same terms: the heights are on
    // the field, the layer's coverage is where the backdrop goes, and
    // what the stamp is left with is the lit half as a picture.
    if in.extra.z != 0.0 {
        return bevel_lit(in);
    }
    var out = gained(field_at(in.uv, vec2f(in.params.x, in.params.y)), in.extra.xy);
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
// What a pixel shows, for a surface blur to judge likeness by: its
// straight colour in the display encoding, held to 0..1, and its alpha —
// the CPU renderer's `bilateral::shown`.
fn shown_of(p: vec4f) -> vec4f {
    if p.a <= 0.0 {
        return vec4f(0.0);
    }
    let s = clamp(p.rgb / p.a, vec3f(0.0), vec3f(1.0));
    return vec4f(to_shown(s.r), to_shown(s.g), to_shown(s.b), p.a);
}

// One pass of a surface blur: the weighted average of the `params.x`
// pixels each side of this one along a row (`params.w` 2) or down a
// column (3), each weighed by a Gaussian of half the radius and by how
// alike it looks — one for the same shown colour, nothing once any
// channel is `params.y` apart. Reads past the page's edge are held at
// it, as the CPU's are at the edge of what it snapshotted.
fn surface_pass(in: ImageOut) -> vec4f {
    let r = i32(in.params.x);
    let threshold = in.params.y;
    let vertical = in.params.w > 2.5;
    let sigma = max(f32(r) / 2.0, 0.5);
    let lo = vec2i(page.lo);
    let hi = vec2i(page.hi) - vec2i(1, 1);
    let here = vec2i(floor(in.uv * page.size));
    let c = shown_of(textureLoad(image, clamp(here, lo, hi), 0));
    var sum = vec4f(0.0);
    var total = 0.0;
    for (var k = -r; k <= r; k = k + 1) {
        var p = here;
        if vertical {
            p.y = p.y + k;
        } else {
            p.x = p.x + k;
        }
        let px = textureLoad(image, clamp(p, lo, hi), 0);
        let d = abs(shown_of(px) - c);
        let apart = max(max(d.r, d.g), max(d.b, d.a));
        let near = exp(-f32(k * k) / (2.0 * sigma * sigma));
        let weight = near * max(1.0 - apart / threshold, 0.0);
        sum = sum + weight * px;
        total = total + weight;
    }
    return sum / max(total, 1e-6);
}

// A pixelate whose grid lies at an angle on the page, in one pass: the
// average of every pixel within `extra.x` of this one, and on the page,
// that lands in the same block — each asked where its middle goes mapped
// back through the inverse (`grad`, about the origin in `params.xy`),
// over the side (`params.z`), the CPU renderer's `block_averages` to the
// letter.
fn turned_block(in: ImageOut) -> vec4f {
    let here = floor(in.uv * page.size);
    let cell = turned_cell(here, in);
    let r = i32(in.extra.x);
    let lo = vec2i(page.lo);
    let hi = vec2i(page.hi);
    var sum = vec4f(0.0);
    var n = 0.0;
    for (var j = -r; j <= r; j = j + 1) {
        for (var i = -r; i <= r; i = i + 1) {
            let q = vec2i(here) + vec2i(i, j);
            if q.x < lo.x || q.y < lo.y || q.x >= hi.x || q.y >= hi.y {
                continue;
            }
            let c = turned_cell(vec2f(q), in);
            if c.x != cell.x || c.y != cell.y {
                continue;
            }
            sum = sum + textureLoad(image, q, 0);
            n = n + 1.0;
        }
    }
    return sum / max(n, 1.0);
}

fn turned_cell(p: vec2f, in: ImageOut) -> vec2f {
    let dx = (p.x + 0.5) - in.params.x;
    let dy = (p.y + 0.5) - in.params.y;
    let lx = in.grad.x * dx + in.grad.z * dy;
    let ly = in.grad.y * dx + in.grad.w * dy;
    return floor(vec2f(lx, ly) / in.params.z);
}

@fragment
fn fs_block(in: ImageOut) -> @location(0) vec4f {
    if in.params.w > 4.5 {
        // The turned grid's second pass: the first's answer, carried.
        return textureLoad(image, vec2i(floor(in.uv * page.size)), 0);
    }
    if in.params.w > 3.5 {
        return turned_block(in);
    }
    if in.params.w > 1.5 {
        return surface_pass(in);
    }
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
    // A high pass: the difference from the blur on a grey of half of
    // what is covered, held at nothing below, weighed in as a blur is.
    if in.params.y != 0.0 {
        let detail = max(vec3f(0.5 * was.a) + was.rgb - soft.rgb, vec3f(0.0));
        return mix(was, vec4f(detail, was.a), weight);
    }
    if in.params.x == 0.0 {
        return mix(was, soft, weight);
    }
    // Premultiplied, held at nothing below and let run above the alpha
    // they are a share of: light does not stop at white here, and
    // holding them there meant sharpening a ground two stops up brought
    // it back to one. The layer's weight goes into the amount rather
    // than into a mix, which is where the two readings differ.
    let amount = in.params.x * weight;
    let out = max(was.rgb + (was.rgb - soft.rgb) * amount, vec3f(0.0));
    return vec4f(out, was.a);
}
