// One quad per shape, in document space; the fragment finds its coverage
// from the shape's own signed distance, so an edge is as smooth as the
// pixel it lands on rather than as coarse as the mesh.

struct VsOut {
    @builtin(position) pos: vec4f,
    @location(0) local: vec2f,
    @location(1) @interpolate(flat) params: vec4f,
    @location(2) @interpolate(flat) color: vec4f,
};

struct Page {
    size: vec2f,
    pad: vec2f,
};

@group(0) @binding(0) var<uniform> page: Page;

@vertex
fn vs(
    @location(0) doc: vec2f,
    @location(1) local: vec2f,
    @location(2) params: vec4f,
    @location(3) color: vec4f,
) -> VsOut {
    var out: VsOut;
    // Document pixels to clip space, y downwards as the document has it.
    let ndc = vec2f(doc.x / page.size.x * 2.0 - 1.0, 1.0 - doc.y / page.size.y * 2.0);
    out.pos = vec4f(ndc, 0.0, 1.0);
    out.local = local;
    out.params = params;
    out.color = color;
    return out;
}

// Signed distance to a rounded rectangle whose top-left is the origin.
fn rect_distance(p: vec2f, size: vec2f, r: f32) -> f32 {
    let half = size * 0.5;
    let q = abs(p - half) - (half - vec2f(r, r));
    return length(max(q, vec2f(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - r;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4f {
    var d: f32;
    if in.params.w < 0.5 {
        d = rect_distance(in.local, in.params.xy, in.params.z);
    } else {
        // An ellipse's implicit function, divided by its own gradient:
        // the first-order distance to the rim, which is what an edge a
        // pixel wide needs.
        let r = in.params.xy * 0.5;
        let k = (in.local - r) / r;
        let f = dot(k, k) - 1.0;
        let g = 2.0 * vec2f(k.x / r.x, k.y / r.y);
        d = f / max(length(g), 1e-6);
    }
    // The band is one device pixel wide however the shape is transformed:
    // fwidth measures the distance's own rate of change on the screen.
    let cov = clamp(0.5 - d / max(fwidth(d), 1e-6), 0.0, 1.0);
    return in.color * cov;
}
