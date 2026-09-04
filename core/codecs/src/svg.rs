//! SVG export: vector layers export as live SVG elements (the point of a
//! vector editor), rasters embed as data-URI images, text as `<text>`.
//!
//! Faithful subset, honestly scoped: groups carry opacity and blend
//! (`mix-blend-mode`); adjustment and filter layers have no SVG equivalent
//! that matches our compositing and are skipped with a comment; a mask
//! travels as a picture of what it lets through, since SVG has no mask
//! shaped like ours — the artwork under it stays live.

use chitrakar_color::AuthoredColor;
use chitrakar_doc::{
    BlendMode, DocError, Document, Gradient, NodeId, NodeKind, Transform, VectorShape,
};
use std::fmt::Write;

/// Serialize the document's renderable tree as an SVG string.
pub fn export_svg(doc: &Document) -> Result<String, DocError> {
    let mut out = String::new();
    let _ = write!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">"#,
        w = doc.meta.width,
        h = doc.meta.height
    );
    out.push('\n');
    // Gradients are referenced by url(#id), so the body is written first and
    // the defs it asked for are spliced in ahead of it.
    let mut body = String::new();
    let mut defs = String::new();
    write_children(doc, doc.root(), &mut body, 1, &mut defs)?;
    if !defs.is_empty() {
        let _ = write!(out, "  <defs>\n{defs}  </defs>\n");
    }
    out.push_str(&body);
    out.push_str("</svg>\n");
    Ok(out)
}

fn write_children(
    doc: &Document,
    group: NodeId,
    out: &mut String,
    depth: usize,
    defs: &mut String,
) -> Result<(), DocError> {
    for &child in doc.children_of(group)? {
        write_node(doc, child, out, depth, defs)?;
    }
    Ok(())
}

/// One layer's markup, at `depth` levels of indentation. Pulled out of
/// the walk so a copy of a layer can ask for the layer's own markup
/// again, inside the copy's place.
fn write_node(
    doc: &Document,
    child: NodeId,
    out: &mut String,
    depth: usize,
    defs: &mut String,
) -> Result<(), DocError> {
    {
        let node = doc.node(child)?;
        if !node.visible || node.opacity <= 0.0 {
            return Ok(());
        }
        let pad = "  ".repeat(depth);
        let common = common_attrs(node);
        // A mask is authored in the space the layer sits in, so it goes
        // on a wrapper that carries no transform of its own: SVG reads a
        // userSpaceOnUse mask in the space in force where it is
        // referenced, and the layer's own transform belongs inside that.
        let masked = mask_attrs(doc, child, defs);
        // A layer confined to the one below it is the same idea one level
        // out: SVG has no clipping to another layer's alpha, so what that
        // layer lets through travels as a mask of its own, on a wrapper
        // outside the layer's own.
        let confined = clip_attrs(doc, child, defs);
        if !confined.is_empty() {
            let _ = writeln!(out, "{pad}<g{confined}>");
        }
        if !masked.is_empty() {
            let _ = writeln!(out, "{pad}<g{masked}>");
        }
        match &node.kind {
            NodeKind::Group => {
                let _ = writeln!(out, "{pad}<g{common}>");
                write_children(doc, child, out, depth + 1, defs)?;
                let _ = writeln!(out, "{pad}</g>");
            }
            NodeKind::Instance { of, .. } => {
                // A copy is the original's markup again, inside the
                // copy's own transform, with the original's own
                // placement undone: SVG has <use>, but it would carry
                // that placement with it.
                let Some(back) = doc
                    .node(*of)
                    .ok()
                    .and_then(|m| chitrakar_render::invert(m.transform))
                else {
                    return Ok(());
                };
                // Adding zero turns the negative zeros an inverse
                // produces back into plain ones: the same number, but
                // "-0" in the markup reads as a mistake.
                let z = |v: f32| v + 0.0;
                let undo = format!(
                    r#" transform="matrix({} {} {} {} {} {})""#,
                    z(back.a),
                    z(back.b),
                    z(back.c),
                    z(back.d),
                    z(back.e),
                    z(back.f)
                );
                let _ = writeln!(out, "{pad}<g{common}>");
                let _ = writeln!(out, "{pad}  <g{undo}>");
                write_node(doc, *of, out, depth + 2, defs)?;
                let _ = writeln!(out, "{pad}  </g>");
                let _ = writeln!(out, "{pad}</g>");
            }
            NodeKind::Artboard {
                width,
                height,
                background,
            } => {
                // A frame is a group cut to a rectangle, which SVG says
                // with a clipPath in the frame's own space — so it goes
                // on the same element that carries the frame's transform.
                let name = format!("frame{}", child.0);
                let _ = writeln!(
                    defs,
                    r#"<clipPath id="{name}"><rect width="{width}" height="{height}"/></clipPath>"#
                );
                let _ = writeln!(out, r#"{pad}<g{common} clip-path="url(#{name})">"#);
                if let Some(color) = background {
                    let ground = paint_attrs(doc, Some(color), None, None, child, defs, false);
                    let _ = writeln!(
                        out,
                        r#"{pad}  <rect width="{width}" height="{height}"{ground}/>"#
                    );
                }
                write_children(doc, child, out, depth + 1, defs)?;
                let _ = writeln!(out, "{pad}</g>");
            }
            NodeKind::Vector {
                shape,
                fill,
                stroke,
                gradient,
            } => {
                // A rect's or an ellipse's stroke is a band lying inside
                // its edge, where SVG's own stroke straddles it — an
                // eight-wide border would come out four in and four out,
                // and the shape four wider all round than the engine
                // drew it. So the two are written as two elements: the
                // shape with its fill, and the same shape drawn half a
                // width smaller carrying the stroke, whose centred band
                // then lands exactly where the inner one was.
                // SVG straddles an outline with a stroke and has no way
                // to say otherwise, so a band lying to one side is drawn
                // as a stroke on a shape moved half a width that way —
                // which lands the band exactly where the engine puts it.
                // How far to move it, towards the middle of the shape:
                let offset =
                    stroke
                        .as_ref()
                        .and_then(|s| match chitrakar_doc::stroke_align(shape, s) {
                            chitrakar_doc::StrokeAlign::Inside => Some(s.width / 2.0),
                            chitrakar_doc::StrokeAlign::Outside => Some(-s.width / 2.0),
                            chitrakar_doc::StrokeAlign::Centre => None,
                        });
                let inner = stroke.as_ref().filter(|_| offset.is_some());
                let offset = offset.unwrap_or(0.0);
                let paint = paint_attrs(
                    doc,
                    fill.as_ref(),
                    if inner.is_some() {
                        None
                    } else {
                        stroke.as_ref()
                    },
                    gradient.as_ref(),
                    child,
                    defs,
                    inner.is_none(),
                );
                match shape {
                    VectorShape::Rect {
                        width,
                        height,
                        radius,
                    } => {
                        // SVG rounds corners with rx, clamped the same way
                        // the renderer clamps its own radius.
                        let clamp = |r: f32, w: f32, h: f32| {
                            r.max(0.0).min((w / 2.0).min(h / 2.0).max(0.0))
                        };
                        let round = |r: f32| {
                            if r > 0.0 {
                                format!(r#" rx="{r}""#)
                            } else {
                                String::new()
                            }
                        };
                        let r = clamp(*radius, *width, *height);
                        match inner {
                            None => {
                                let _ = writeln!(
                                    out,
                                    r#"{pad}<rect width="{width}" height="{height}"{}{common}{paint}/>"#,
                                    round(r)
                                );
                            }
                            Some(band) => {
                                let (iw, ih) = (width - offset * 2.0, height - offset * 2.0);
                                let line =
                                    paint_attrs(doc, None, Some(band), None, child, defs, false);
                                let _ = writeln!(out, "{pad}<g{common}>");
                                let _ = writeln!(
                                    out,
                                    r#"{pad}  <rect width="{width}" height="{height}"{}{paint}/>"#,
                                    round(r)
                                );
                                if iw > 0.0 && ih > 0.0 {
                                    let _ = writeln!(
                                        out,
                                        r#"{pad}  <rect x="{offset}" y="{offset}" width="{iw}" height="{ih}"{}{line}/>"#,
                                        round(clamp(r - offset, iw, ih))
                                    );
                                }
                                let _ = writeln!(out, "{pad}</g>");
                            }
                        }
                    }
                    VectorShape::Ellipse { rx, ry } => match inner {
                        None => {
                            let _ = writeln!(
                                out,
                                r#"{pad}<ellipse cx="{rx}" cy="{ry}" rx="{rx}" ry="{ry}"{common}{paint}/>"#
                            );
                        }
                        Some(band) => {
                            let (ix, iy) = (rx - offset, ry - offset);
                            let line = paint_attrs(doc, None, Some(band), None, child, defs, false);
                            let _ = writeln!(out, "{pad}<g{common}>");
                            let _ = writeln!(
                                out,
                                r#"{pad}  <ellipse cx="{rx}" cy="{ry}" rx="{rx}" ry="{ry}"{paint}/>"#
                            );
                            if ix > 0.0 && iy > 0.0 {
                                let _ = writeln!(
                                    out,
                                    r#"{pad}  <ellipse cx="{rx}" cy="{ry}" rx="{ix}" ry="{iy}"{line}/>"#
                                );
                            }
                            let _ = writeln!(out, "{pad}</g>");
                        }
                    },
                    VectorShape::Path {
                        points,
                        closed,
                        smooth,
                        handles,
                        subpaths,
                    } => {
                        // With handles the path exports as real cubic
                        // segments, so a curve stays a curve downstream
                        // instead of arriving as a flattened polyline.
                        let curved = handles.len() == points.len()
                            && handles.iter().any(|h| h.iter().any(|v| v.abs() > 1e-6));
                        let mut d = String::new();
                        if curved && points.len() >= 2 {
                            let _ = write!(d, "M{},{}", points[0][0], points[0][1]);
                            let segments = if *closed {
                                points.len()
                            } else {
                                points.len() - 1
                            };
                            for i in 0..segments {
                                let j = (i + 1) % points.len();
                                let (a, b) = (points[i], points[j]);
                                let _ = write!(
                                    d,
                                    " C{},{} {},{} {},{}",
                                    a[0] + handles[i][2],
                                    a[1] + handles[i][3],
                                    b[0] + handles[j][0],
                                    b[1] + handles[j][1],
                                    b[0],
                                    b[1]
                                );
                            }
                        } else {
                            for (i, p) in points.iter().enumerate() {
                                let _ = write!(
                                    d,
                                    "{}{},{}",
                                    if i == 0 { "M" } else { " L" },
                                    p[0],
                                    p[1]
                                );
                            }
                        }
                        if *closed {
                            d.push_str(" Z");
                        }
                        // Extra rings become extra subpaths in the same d,
                        // which is what fill-rule="evenodd" needs to see to
                        // cut a hole rather than paint over it.
                        for ring in subpaths {
                            for (i, p) in ring.iter().enumerate() {
                                let _ = write!(
                                    d,
                                    "{}{},{}",
                                    if i == 0 { " M" } else { " L" },
                                    p[0],
                                    p[1]
                                );
                            }
                            d.push_str(" Z");
                        }
                        let rule = if subpaths.is_empty() {
                            ""
                        } else {
                            r#" fill-rule="evenodd""#
                        };
                        let smooth_note = if *smooth {
                            " data-chitrakar-smooth=\"true\""
                        } else {
                            ""
                        };
                        let _ = writeln!(
                            out,
                            r#"{pad}<path d="{d}"{common}{paint}{rule}{smooth_note}/>"#
                        );
                    }
                }
            }
            NodeKind::Raster(raster) => {
                if let Some(res) = doc.resource(&raster.resource_id) {
                    if !res.rgba8.is_empty() {
                        if let Ok(png) = crate::encode_png(res.width, res.height, &res.rgba8) {
                            let _ = writeln!(
                                out,
                                r#"{pad}<image width="{}" height="{}"{common} href="data:image/png;base64,{}"/>"#,
                                res.width,
                                res.height,
                                base64(&png)
                            );
                        }
                    }
                }
            }
            // SVG has no brush, so a paint layer travels as the pixels
            // it paints, placed where its paint actually sits.
            NodeKind::Paint { .. } => {
                if let Ok(Some(painted)) = chitrakar_render::paint_pixels(doc, child) {
                    let (w, h) = (painted.width, painted.height);
                    let [x, y] = painted.origin;
                    if let Ok(png) = crate::encode_png(w, h, &painted.rgba8) {
                        let _ = writeln!(out, "{pad}<g{common}>");
                        let _ = writeln!(
                            out,
                            r#"{pad}  <image width="{w}" height="{h}" transform="translate({x} {y})" href="data:image/png;base64,{}"/>"#,
                            base64(&png)
                        );
                        let _ = writeln!(out, "{pad}</g>");
                    }
                }
            }
            NodeKind::Text(spec) => {
                // SVG text is baseline-anchored and single-line; our node
                // origin is the block's top and its text can hold newlines
                // and wrap. The renderer says where every line lands, so
                // each becomes a tspan on its own baseline, aligned by
                // text-anchor at the same x the raster aligns it at, and
                // the font-size is the em the face is really scaled to.
                let block = chitrakar_render::text::set(spec);
                let track = spec.letter_spacing * spec.size;
                let spacing = if track.abs() > 1e-4 {
                    format!(r#" letter-spacing="{track:.3}""#)
                } else {
                    String::new()
                };
                let style = if spec.italic {
                    r#" font-style="italic""#
                } else {
                    ""
                };
                let weight = if spec.bold {
                    r#" font-weight="bold""#
                } else {
                    ""
                };
                let decoration = match (spec.underline, spec.strike) {
                    (true, true) => r#" text-decoration="underline line-through""#,
                    (true, false) => r#" text-decoration="underline""#,
                    (false, true) => r#" text-decoration="line-through""#,
                    (false, false) => "",
                };
                let (anchor, x) = match spec.align {
                    chitrakar_doc::TextAlign::Left => ("", block.inset),
                    chitrakar_doc::TextAlign::Center => {
                        (r#" text-anchor="middle""#, block.inset + block.inner / 2.0)
                    }
                    chitrakar_doc::TextAlign::Right => {
                        (r#" text-anchor="end""#, block.inset + block.inner)
                    }
                };
                let _ = writeln!(
                    out,
                    r#"{pad}<text font-family="{}, sans-serif" font-size="{:.2}"{spacing}{style}{weight}{decoration}{anchor}{common} fill="{}" xml:space="preserve">"#,
                    if spec.font.is_empty() {
                        "DejaVu Sans"
                    } else {
                        spec.font.as_str()
                    },
                    block.em,
                    color_hex(doc, spec.fill),
                );
                // No indentation inside: the block preserves its space
                // (an indent typed into a line is meant), so the only
                // whitespace between the tags is the newline.
                // Along a guide the text is one run on a textPath: the
                // guide goes into the defs as the renderer flattens it,
                // with the offset as startOffset.
                if let Some((points, closed)) = chitrakar_render::text::guide_points(spec) {
                    let name = format!("guide{}", child.0);
                    let mut d = String::new();
                    for (i, p) in points.iter().enumerate() {
                        let _ = write!(
                            d,
                            "{}{:.2},{:.2}",
                            if i == 0 { "M" } else { " L" },
                            p[0],
                            p[1]
                        );
                    }
                    if closed {
                        d.push_str(" Z");
                    }
                    let _ = writeln!(defs, r#"<path id="{name}" d="{d}"/>"#);
                    let _ = writeln!(
                        out,
                        r##"<textPath href="#{name}" startOffset="{:.2}">{}</textPath>"##,
                        spec.along_offset,
                        escape_xml(&spec.text.replace('\n', " "))
                    );
                    let _ = writeln!(out, "</text>");
                } else {
                    for (i, set) in block.lines.iter().enumerate() {
                        let y = block.ascent + i as f32 * block.step;
                        // A line no style run touches is one tspan, as it
                        // always was. One a run crosses is cut where the
                        // run starts and stops, each piece carrying only
                        // what its run changes — SVG inherits the rest
                        // from the text element around them, and the x is
                        // given once so the pieces run on from each other.
                        let cuts = line_pieces(spec, set);
                        for (n, (piece, run)) in cuts.iter().enumerate() {
                            let at = if n == 0 {
                                format!(r#" x="{x:.2}" y="{y:.2}""#)
                            } else {
                                String::new()
                            };
                            let _ = writeln!(
                                out,
                                "<tspan{at}{}>{}</tspan>",
                                run_attrs(doc, spec, *run),
                                escape_xml(piece)
                            );
                        }
                    }
                    let _ = writeln!(out, "</text>");
                }
            }
            // These three are all changes to what is under them rather
            // than pictures of their own, and SVG has no equivalent that
            // composites the way ours does.
            NodeKind::Adjustment(_) | NodeKind::Filter(_) | NodeKind::Clone { .. } => {
                let _ = writeln!(
                    out,
                    "{pad}<!-- {} layer '{}' has no SVG equivalent; omitted -->",
                    match node.kind {
                        NodeKind::Adjustment(_) => "adjustment",
                        NodeKind::Filter(_) => "filter",
                        _ => "clone",
                    },
                    escape_xml(&node.name)
                );
            }
        }
        if !masked.is_empty() {
            let _ = writeln!(out, "{pad}</g>");
        }
        if !confined.is_empty() {
            let _ = writeln!(out, "{pad}</g>");
        }
    }
    Ok(())
}

/// A `mask="url(#…)"` attribute, with the mask itself written into the
/// defs as a picture of what it lets through: white with the coverage in
/// its alpha, which reads the same whether the mask is taken by its
/// luminance or by its alpha. Empty when the layer has no mask.
fn mask_attrs(doc: &Document, id: NodeId, defs: &mut String) -> String {
    let Ok(Some(m)) = chitrakar_render::mask_pixels(doc, id) else {
        return String::new();
    };
    let Ok(png) = crate::encode_png(m.width, m.height, &m.rgba8) else {
        return String::new();
    };
    let (w, h) = (m.width, m.height);
    let [x, y] = m.origin;
    let name = format!("mask{}", id.0);
    let _ = writeln!(
        defs,
        r#"<mask id="{name}" maskUnits="userSpaceOnUse" x="{x}" y="{y}" width="{w}" height="{h}"><image x="{x}" y="{y}" width="{w}" height="{h}" href="data:image/png;base64,{}"/></mask>"#,
        base64(&png)
    );
    format!(r#" mask="url(#{name})""#)
}

/// The same for a clipped layer, whose mask is the picture the layer
/// below it makes. Empty when the layer is not clipped to anything.
fn clip_attrs(doc: &Document, id: NodeId, defs: &mut String) -> String {
    let Ok(Some(m)) = chitrakar_render::clip_pixels(doc, id) else {
        return String::new();
    };
    let Ok(png) = crate::encode_png(m.width, m.height, &m.rgba8) else {
        return String::new();
    };
    let (w, h) = (m.width, m.height);
    let [x, y] = m.origin;
    let name = format!("clip{}", id.0);
    let _ = writeln!(
        defs,
        r#"<mask id="{name}" maskUnits="userSpaceOnUse" x="{x}" y="{y}" width="{w}" height="{h}"><image x="{x}" y="{y}" width="{w}" height="{h}" href="data:image/png;base64,{}"/></mask>"#,
        base64(&png)
    );
    format!(r#" mask="url(#{name})""#)
}

/// The CSS name for a blend mode: the spec's, hyphenated where it has
/// two words.
fn css_blend(mode: BlendMode) -> &'static str {
    match mode {
        BlendMode::Normal => "normal",
        BlendMode::Multiply => "multiply",
        BlendMode::Screen => "screen",
        BlendMode::Overlay => "overlay",
        BlendMode::Darken => "darken",
        BlendMode::Lighten => "lighten",
        BlendMode::ColorDodge => "color-dodge",
        BlendMode::ColorBurn => "color-burn",
        BlendMode::HardLight => "hard-light",
        BlendMode::SoftLight => "soft-light",
        BlendMode::Difference => "difference",
        BlendMode::Exclusion => "exclusion",
        BlendMode::Hue => "hue",
        BlendMode::Saturation => "saturation",
        BlendMode::Color => "color",
        BlendMode::Luminosity => "luminosity",
    }
}

fn common_attrs(node: &chitrakar_doc::Node) -> String {
    let mut s = String::new();
    let t = node.transform;
    if t != Transform::default() {
        let _ = write!(
            s,
            r#" transform="matrix({} {} {} {} {} {})""#,
            t.a, t.b, t.c, t.d, t.e, t.f
        );
    }
    if node.opacity < 1.0 {
        let _ = write!(s, r#" opacity="{}""#, node.opacity);
    }
    match node.blend {
        BlendMode::Normal => {}
        // The spec's own names, which are what CSS calls them too, so a
        // blend travels as itself.
        other => {
            let _ = write!(s, r#" style="mix-blend-mode:{}""#, css_blend(other));
        }
    }
    s
}

#[allow(clippy::too_many_arguments)]
fn paint_attrs(
    doc: &Document,
    fill: Option<&AuthoredColor>,
    stroke: Option<&chitrakar_doc::Stroke>,
    gradient: Option<&Gradient>,
    id: NodeId,
    defs: &mut String,
    // Whether the stroke has ends and corners of its own to say
    // something about: true for a path's line, false for the band lying
    // inside a rect's or an ellipse's closed outline.
    ends: bool,
) -> String {
    let mut s = String::new();
    // A gradient paints in place of the flat fill, and exports live: our
    // stops are already in objectBoundingBox units, which is SVG's default.
    if let Some(g) = gradient {
        if !g.stops().is_empty() {
            let name = format!("chitrakar-grad-{}", id.0);
            write_gradient_def(doc, g, &name, defs);
            let _ = write!(s, r##" fill="url(#{name})""##);
            if let Some(stroke) = stroke {
                let _ = write!(
                    s,
                    r#" stroke="{}" stroke-width="{}"{}"#,
                    color_hex(doc, stroke.color),
                    stroke.width,
                    stroke_attrs(doc, stroke, ends, id, defs)
                );
            }
            return s;
        }
    }
    match fill {
        Some(c) => {
            let _ = write!(s, r#" fill="{}""#, color_hex(doc, *c));
            let a = color_alpha(*c);
            if a < 1.0 {
                let _ = write!(s, r#" fill-opacity="{a}""#);
            }
        }
        None => s.push_str(r#" fill="none""#),
    }
    if let Some(stroke) = stroke {
        let _ = write!(
            s,
            r#" stroke="{}" stroke-width="{}"{}"#,
            color_hex(doc, stroke.color),
            stroke.width,
            stroke_attrs(doc, stroke, ends, id, defs)
        );
    }
    s
}

/// What a stroke says about itself beyond its colour and width: the dash
/// pattern (the same list of lengths on and off the document keeps), and
/// how it ends and turns.
///
/// SVG takes butt ends and mitred corners as read and so writes nothing
/// for them; the rest are said out loud. The miter limit is four in both,
/// so that too goes without saying.
fn stroke_attrs(
    doc: &Document,
    stroke: &chitrakar_doc::Stroke,
    ends: bool,
    id: NodeId,
    defs: &mut String,
) -> String {
    let mut s = String::new();
    if !stroke.dash.is_empty() && stroke.dash.iter().any(|d| *d > 0.0) {
        let lengths: Vec<String> = stroke.dash.iter().map(|d| d.to_string()).collect();
        let _ = write!(s, r#" stroke-dasharray="{}""#, lengths.join(" "));
    }
    if !ends {
        return s;
    }
    match stroke.cap {
        chitrakar_doc::StrokeCap::Butt => {}
        chitrakar_doc::StrokeCap::Round => s.push_str(r#" stroke-linecap="round""#),
        chitrakar_doc::StrokeCap::Square => s.push_str(r#" stroke-linecap="square""#),
    }
    match stroke.join {
        chitrakar_doc::StrokeJoin::Miter => {}
        chitrakar_doc::StrokeJoin::Round => s.push_str(r#" stroke-linejoin="round""#),
        chitrakar_doc::StrokeJoin::Bevel => s.push_str(r#" stroke-linejoin="bevel""#),
    }
    // What sits at either end travels as a real marker, so a reader can
    // still take it off or restyle it — a picture of an arrowhead would
    // be a picture. It is written in units of the stroke's width, which
    // is what sizes one from the line here too.
    for (marker, at_start) in [(stroke.start_marker, true), (stroke.end_marker, false)] {
        let Some(shape) = marker_path(marker, at_start) else {
            continue;
        };
        let name = format!(
            "chitrakar-{}-{}",
            if at_start { "start" } else { "end" },
            id.0
        );
        let (l, r) = (chitrakar_doc::MARKER_LENGTH, chitrakar_doc::MARKER_REACH);
        let _ = write!(
            defs,
            concat!(
                r#"    <marker id="{name}" markerUnits="strokeWidth" markerWidth="{w}""#,
                r#" markerHeight="{h}" refX="{rx}" refY="{ry}" orient="auto">"#,
                "\n      <path d=\"{shape}\" fill=\"{fill}\"/>\n    </marker>\n"
            ),
            name = name,
            w = l,
            h = r * 2.0,
            // The tip is the point the marker is placed by: at the
            // line's own end for a head, at the middle for a bar or a
            // dot, which is why each shape names its own.
            rx = if at_start { 0.0 } else { l },
            ry = r,
            shape = shape,
            fill = color_hex(doc, stroke.color),
        );
        let _ = write!(
            s,
            r##" marker-{}="url(#{name})""##,
            if at_start { "start" } else { "end" }
        );
    }
    s
}

/// One marker's outline in a box `MARKER_LENGTH` long and twice
/// `MARKER_REACH` tall, in units of the stroke's width.
///
/// SVG turns a marker to the direction the line travels, which points
/// away from the line at its last point and into it at its first — so
/// the two ends are drawn as mirror images rather than being placed
/// differently.
fn marker_path(marker: chitrakar_doc::Marker, at_start: bool) -> Option<String> {
    let (l, r) = (chitrakar_doc::MARKER_LENGTH, chitrakar_doc::MARKER_REACH);
    // Where the tip is, and where the body is measured to.
    let (tip, base) = if at_start { (0.0, l) } else { (l, 0.0) };
    Some(match marker {
        chitrakar_doc::Marker::None => return None,
        chitrakar_doc::Marker::Arrow => format!("M{tip},{r} L{base},0 L{base},{h} z", h = r * 2.0),
        // Half a width either side of the point it is placed on.
        chitrakar_doc::Marker::Bar => {
            let (a, b) = (tip - 0.5, tip + 0.5);
            format!("M{a},0 L{b},0 L{b},{h} L{a},{h} z", h = r * 2.0)
        }
        // A disc, as four quarter arcs about the point.
        chitrakar_doc::Marker::Dot => format!(
            "M{x},{y} m-{r},0 a{r},{r} 0 1,0 {d},0 a{r},{r} 0 1,0 -{d},0 z",
            x = tip,
            y = r,
            d = r * 2.0
        ),
    })
}

fn write_gradient_def(doc: &Document, g: &Gradient, name: &str, defs: &mut String) {
    let (open, close) = match g {
        Gradient::Linear { from, to, .. } => (
            format!(
                r#"    <linearGradient id="{name}" x1="{}" y1="{}" x2="{}" y2="{}">"#,
                from[0], from[1], to[0], to[1]
            ),
            "    </linearGradient>",
        ),
        Gradient::Radial { center, radius, .. } => (
            format!(
                r#"    <radialGradient id="{name}" cx="{}" cy="{}" r="{radius}">"#,
                center[0], center[1]
            ),
            "    </radialGradient>",
        ),
    };
    let _ = writeln!(defs, "{open}");
    let mut stops = g.stops().to_vec();
    stops.sort_by(|a, b| {
        a.offset
            .partial_cmp(&b.offset)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for stop in stops {
        let a = color_alpha(stop.color);
        let _ = writeln!(
            defs,
            r#"      <stop offset="{}" stop-color="{}" stop-opacity="{a}"/>"#,
            stop.offset,
            color_hex(doc, stop.color)
        );
    }
    let _ = writeln!(defs, "{close}");
}

/// Resolve an authored color to an sRGB hex, using the document's press
/// profile for CMYK when one is loaded (matching on-canvas rendering).
fn color_hex(doc: &Document, color: AuthoredColor) -> String {
    let px = match (color, doc.cmyk_cms()) {
        (AuthoredColor::Cmyk { c, m, y, k, .. }, Some(cms)) => cms.to_working(c, m, y, k, 1.0),
        _ => {
            // Drop alpha here; it exports separately as fill-opacity.
            let opaque = match color {
                AuthoredColor::Srgb { r, g, b, .. } => AuthoredColor::Srgb { r, g, b, a: 1.0 },
                AuthoredColor::Cmyk { c, m, y, k, .. } => {
                    AuthoredColor::Cmyk { c, m, y, k, a: 1.0 }
                }
            };
            chitrakar_color::to_working(opaque)
        }
    };
    let [r, g, b, _] = px.to_srgb8();
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn color_alpha(color: AuthoredColor) -> f32 {
    match color {
        AuthoredColor::Srgb { a, .. } | AuthoredColor::Cmyk { a, .. } => a,
    }
}

/// One set line cut where its style runs start and stop: each piece of
/// the line with the run that governs it.
fn line_pieces<'a>(
    spec: &chitrakar_doc::TextSpec,
    line: &'a chitrakar_render::text::SetLine,
) -> Vec<(&'a str, Option<usize>)> {
    if spec.runs.is_empty() {
        return vec![(line.text.as_str(), None)];
    }
    let (at, len) = (line.at, line.text.len());
    spec.pieces()
        .into_iter()
        .filter_map(|(s, e, run)| {
            let (s, e) = (s.max(at), e.min(at + len));
            (s < e).then(|| (&line.text[s - at..e - at], run))
        })
        .collect()
}

/// What a tspan has to say for its run: only what the run changes, since
/// SVG inherits the rest from the text element the tspans sit in.
fn run_attrs(doc: &Document, spec: &chitrakar_doc::TextSpec, run: Option<usize>) -> String {
    let Some(run) = run.and_then(|i| spec.runs.get(i)) else {
        return String::new();
    };
    let mut out = String::new();
    if let Some(fill) = run.fill {
        let _ = write!(out, r#" fill="{}""#, color_hex(doc, fill));
    }
    if let Some(bold) = run.bold {
        let _ = write!(
            out,
            r#" font-weight="{}""#,
            if bold { "bold" } else { "normal" }
        );
    }
    if let Some(italic) = run.italic {
        let _ = write!(
            out,
            r#" font-style="{}""#,
            if italic { "italic" } else { "normal" }
        );
    }
    if let Some(font) = &run.font {
        let _ = write!(out, r#" font-family="{}, sans-serif""#, escape_xml(font));
    }
    match (run.underline, run.strike) {
        (Some(true), Some(true)) => out.push_str(r#" text-decoration="underline line-through""#),
        (Some(true), _) => out.push_str(r#" text-decoration="underline""#),
        (_, Some(true)) => out.push_str(r#" text-decoration="line-through""#),
        (Some(false), _) | (_, Some(false)) => out.push_str(r#" text-decoration="none""#),
        (None, None) => {}
    }
    out
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chitrakar_color::ColorMode;
    use chitrakar_doc::{Command, Node};

    const RED: AuthoredColor = AuthoredColor::Srgb {
        r: 1.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };

    fn filled(doc: &mut Document, name: &str, w: f32, h: f32) -> NodeId {
        let root = doc.root();
        let index = doc.children_of(root).unwrap().len();
        let mut rect = Node::vector(
            name,
            VectorShape::Rect {
                width: w,
                height: h,
                radius: 0.0,
            },
        );
        if let NodeKind::Vector { fill, .. } = &mut rect.kind {
            *fill = Some(RED);
        }
        doc.apply(Command::AddNode {
            parent: root,
            index,
            node: Box::new(rect),
        })
        .unwrap();
        doc.children_of(root).unwrap()[index]
    }

    /// The engine draws a rect's or an ellipse's stroke as a band lying
    /// inside its edge; SVG's own stroke straddles it. So the two are
    /// written apart — the shape with its fill, and the same shape half a
    /// width smaller carrying the stroke — which puts the band back where
    /// the engine had it instead of four units wider all round.
    #[test]
    fn an_inner_stroke_is_written_where_the_engine_draws_it() {
        let mut doc = Document::new(80, 80, ColorMode::Rgb);
        let id = filled(&mut doc, "r", 40.0, 40.0);
        let kind = |stroke: Option<chitrakar_doc::Stroke>| chitrakar_doc::NodeKind::Vector {
            shape: VectorShape::Rect {
                width: 40.0,
                height: 40.0,
                radius: 0.0,
            },
            fill: Some(RED),
            stroke,
            gradient: None,
        };
        // With no stroke it stays one element, as it always was.
        doc.apply(Command::SetKind {
            id,
            kind: Box::new(kind(None)),
        })
        .unwrap();
        let plain = export_svg(&doc).unwrap();
        assert!(
            plain.contains(r#"<rect width="40" height="40""#) && !plain.contains("stroke="),
            "{plain}"
        );

        doc.apply(Command::SetKind {
            id,
            kind: Box::new(kind(Some(chitrakar_doc::Stroke {
                color: RED,
                width: 8.0,
                widths: Vec::new(),
                dash: Vec::new(),
                cap: Default::default(),
                join: Default::default(),
                start_marker: Default::default(),
                end_marker: Default::default(),
                align: None,
            }))),
        })
        .unwrap();
        let svg = export_svg(&doc).unwrap();
        assert!(
            svg.contains(r##"<rect width="40" height="40" fill="#ff0000"/>"##),
            "the fill still covers the whole shape: {svg}"
        );
        assert!(
            svg.contains(r#"<rect x="4" y="4" width="32" height="32""#)
                && svg.contains(r#"stroke-width="8""#),
            "and the band's own middle is half a width in: {svg}"
        );
    }

    /// A dash pattern travels as the same lengths SVG names, and a solid
    /// stroke says nothing about dashes at all.
    #[test]
    fn a_dashed_stroke_travels_as_its_pattern() {
        let mut doc = Document::new(80, 80, ColorMode::Rgb);
        let id = filled(&mut doc, "line", 40.0, 40.0);
        let stroked = |dash: Vec<f32>| chitrakar_doc::NodeKind::Vector {
            shape: VectorShape::Rect {
                width: 40.0,
                height: 40.0,
                radius: 0.0,
            },
            fill: None,
            stroke: Some(chitrakar_doc::Stroke {
                color: RED,
                width: 2.0,
                widths: Vec::new(),
                dash,
                cap: Default::default(),
                join: Default::default(),
                start_marker: Default::default(),
                end_marker: Default::default(),
                align: None,
            }),
            gradient: None,
        };
        doc.apply(Command::SetKind {
            id,
            kind: Box::new(stroked(Vec::new())),
        })
        .unwrap();
        assert!(
            !export_svg(&doc).unwrap().contains("dasharray"),
            "a solid stroke says nothing about dashes"
        );
        doc.apply(Command::SetKind {
            id,
            kind: Box::new(stroked(vec![6.0, 3.0])),
        })
        .unwrap();
        assert!(
            export_svg(&doc)
                .unwrap()
                .contains(r#"stroke-dasharray="6 3""#),
            "and a dashed one carries its pattern"
        );
    }

    /// How a line ends and turns travels too — and only when there is
    /// something to say: SVG already takes flat ends and mitred corners
    /// as read, and a rect's or an ellipse's band has no ends at all.
    #[test]
    fn how_a_line_ends_and_turns_travels() {
        let mut doc = Document::new(80, 80, ColorMode::Rgb);
        let id = filled(&mut doc, "line", 40.0, 40.0);
        let line = |cap, join| chitrakar_doc::NodeKind::Vector {
            shape: VectorShape::Path {
                points: vec![[0.0, 0.0], [40.0, 0.0], [40.0, 40.0]],
                closed: false,
                smooth: false,
                handles: Vec::new(),
                subpaths: Vec::new(),
            },
            fill: None,
            stroke: Some(chitrakar_doc::Stroke {
                color: RED,
                width: 4.0,
                widths: Vec::new(),
                dash: Vec::new(),
                cap,
                join,
                start_marker: Default::default(),
                end_marker: Default::default(),
                align: None,
            }),
            gradient: None,
        };
        let exported = |doc: &mut Document, cap, join| {
            doc.apply(Command::SetKind {
                id,
                kind: Box::new(line(cap, join)),
            })
            .unwrap();
            export_svg(doc).unwrap()
        };

        let svg = exported(
            &mut doc,
            chitrakar_doc::StrokeCap::Butt,
            chitrakar_doc::StrokeJoin::Miter,
        );
        assert!(
            !svg.contains("linecap") && !svg.contains("linejoin"),
            "what SVG assumes goes without saying: {svg}"
        );

        let svg = exported(
            &mut doc,
            chitrakar_doc::StrokeCap::Round,
            chitrakar_doc::StrokeJoin::Round,
        );
        assert!(
            svg.contains(r#"stroke-linecap="round""#) && svg.contains(r#"stroke-linejoin="round""#),
            "the rest is said out loud: {svg}"
        );

        let svg = exported(
            &mut doc,
            chitrakar_doc::StrokeCap::Square,
            chitrakar_doc::StrokeJoin::Bevel,
        );
        assert!(
            svg.contains(r#"stroke-linecap="square""#)
                && svg.contains(r#"stroke-linejoin="bevel""#),
            "and so is this: {svg}"
        );

        // A rect's stroke is a band lying inside a closed outline: it has
        // no ends, and its corners are the rect's own.
        doc.apply(Command::SetKind {
            id,
            kind: Box::new(chitrakar_doc::NodeKind::Vector {
                shape: VectorShape::Rect {
                    width: 40.0,
                    height: 40.0,
                    radius: 0.0,
                },
                fill: None,
                stroke: Some(chitrakar_doc::Stroke {
                    color: RED,
                    width: 4.0,
                    widths: Vec::new(),
                    dash: Vec::new(),
                    cap: chitrakar_doc::StrokeCap::Round,
                    join: chitrakar_doc::StrokeJoin::Round,
                    start_marker: Default::default(),
                    end_marker: Default::default(),
                    align: None,
                }),
                gradient: None,
            }),
        })
        .unwrap();
        let svg = export_svg(&doc).unwrap();
        assert!(
            !svg.contains("linecap") && !svg.contains("linejoin"),
            "a band inside an outline says nothing about either: {svg}"
        );
    }

    /// A blend travels under the name the spec gives it, which is what
    /// CSS calls it too — so what a browser shows is what the engine drew.
    #[test]
    fn blends_travel_under_their_own_names() {
        let mut doc = Document::new(60, 60, ColorMode::Rgb);
        let id = filled(&mut doc, "over", 20.0, 20.0);
        for (mode, css) in [
            (BlendMode::Overlay, "overlay"),
            (BlendMode::ColorDodge, "color-dodge"),
            (BlendMode::SoftLight, "soft-light"),
            (BlendMode::Luminosity, "luminosity"),
        ] {
            doc.apply(Command::SetBlendMode { id, blend: mode })
                .unwrap();
            let svg = export_svg(&doc).unwrap();
            assert!(
                svg.contains(&format!("mix-blend-mode:{css}")),
                "{mode:?} travels as {css}: {svg}"
            );
        }
        doc.apply(Command::SetBlendMode {
            id,
            blend: BlendMode::Normal,
        })
        .unwrap();
        assert!(
            !export_svg(&doc).unwrap().contains("mix-blend-mode"),
            "and the plain one says nothing"
        );
    }

    /// A copy travels as the original's markup again, inside the copy's
    /// own place, with the original's own placement undone.
    #[test]
    fn a_copy_travels_as_the_original_drawn_again() {
        let mut doc = Document::new(200, 200, ColorMode::Rgb);
        let master = filled(&mut doc, "master", 40.0, 30.0);
        doc.apply(Command::SetTransform {
            id: master,
            transform: Transform::translation(10.0, 10.0),
        })
        .unwrap();
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 1,
            node: Box::new(Node::instance("copy", master)),
        })
        .unwrap();
        let copy = doc.children_of(root).unwrap()[1];
        doc.apply(Command::SetTransform {
            id: copy,
            transform: Transform::translation(100.0, 60.0),
        })
        .unwrap();
        let svg = export_svg(&doc).unwrap();
        // Two rects of the same size: the original and the copy of it.
        assert_eq!(
            svg.matches(r#"<rect width="40" height="30""#).count(),
            2,
            "the copy is the original drawn again: {svg}"
        );
        assert!(
            svg.contains("matrix(1 0 0 1 -10 -10)"),
            "with the original's own placement undone: {svg}"
        );
    }

    /// A frame travels as a group cut to a rectangle, with its ground
    /// inside the cut.
    #[test]
    fn a_frame_travels_as_a_clipped_group() {
        let mut doc = Document::new(200, 200, ColorMode::Rgb);
        let root = doc.root();
        let mut board = Node::artboard("Artboard 1", 60.0, 40.0, Some(RED));
        board.transform = Transform::translation(20.0, 30.0);
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(board),
        })
        .unwrap();
        let svg = export_svg(&doc).unwrap();
        assert!(
            svg.contains(r#"<clipPath id="frame"#),
            "the frame's rectangle is in the defs: {svg}"
        );
        assert!(
            svg.contains(r#"clip-path="url(#frame"#),
            "and the group is cut by it"
        );
        assert!(
            svg.contains(r#"<rect width="60" height="40""#),
            "with its ground painted inside"
        );
    }

    /// SVG has no clipping to another layer's alpha, so a clipped layer
    /// travels as a mask made of the layer it is clipped to.
    #[test]
    fn a_clipped_layer_travels_as_a_mask_of_the_layer_below() {
        let mut doc = Document::new(200, 200, ColorMode::Rgb);
        filled(&mut doc, "under", 80.0, 60.0);
        let over = filled(&mut doc, "over", 200.0, 200.0);
        assert!(
            !export_svg(&doc).unwrap().contains("<mask"),
            "nothing clipped, no mask element"
        );

        doc.apply(Command::SetClipped {
            id: over,
            clipped: true,
        })
        .unwrap();
        let svg = export_svg(&doc).unwrap();
        assert!(
            svg.contains(r#"<mask id="clip"#) && svg.contains(r#"mask="url(#clip"#),
            "the layer below travels as a mask the upper one references"
        );
        // It is the lower layer's box that is masked out, not the page.
        let at = svg.find(r#"<mask id="clip"#).unwrap();
        let head = &svg[at..at + 140];
        assert!(
            head.contains(r#"width="80""#) && head.contains(r#"height="60""#),
            "over the box of what it is clipped to: {head}"
        );
    }

    /// A mask travels as a picture of what it lets through, on a wrapper
    /// that carries no transform: SVG reads a userSpaceOnUse mask in the
    /// space in force where it is referenced, and the layer's own
    /// transform belongs inside that.
    #[test]
    fn a_mask_travels_as_what_it_lets_through() {
        let mut doc = Document::new(200, 200, ColorMode::Rgb);
        let root = doc.root();
        let mut rect = Node::vector(
            "photo",
            VectorShape::Rect {
                width: 80.0,
                height: 60.0,
                radius: 0.0,
            },
        );
        if let NodeKind::Vector { fill, .. } = &mut rect.kind {
            *fill = Some(RED);
        }
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(rect),
        })
        .unwrap();
        let id = doc.children_of(root).unwrap()[0];
        doc.apply(Command::SetTransform {
            id,
            transform: Transform::translation(40.0, 40.0),
        })
        .unwrap();

        // With no mask there is nothing extra to say.
        let plain = export_svg(&doc).unwrap();
        assert!(!plain.contains("<mask"), "no mask, no mask element");

        doc.apply(Command::SetMask {
            id,
            mask: Some(Box::new(chitrakar_doc::Mask {
                kind: chitrakar_doc::MaskKind::Painted {
                    strokes: vec![chitrakar_doc::PaintStroke {
                        points: vec![[80.0, 70.0]],
                        radii: vec![10.0],
                        color: RED,
                        softness: 0.0,
                        erase: true,
                        source: [0.0, 0.0],
                        heal: false,
                    }],
                },
                invert: false,
            })),
        })
        .unwrap();
        let svg = export_svg(&doc).unwrap();
        assert!(svg.contains(r#"<mask id="mask"#), "the mask is in the defs");
        assert!(
            svg.contains(r#"maskUnits="userSpaceOnUse""#),
            "measured in the space the layer sits in"
        );
        assert!(
            svg.contains(r#"mask="url(#mask"#),
            "and the layer references it"
        );
        // The wrapper carrying the mask has no transform of its own; the
        // rect inside it keeps the one it had.
        let at = svg.find(r#"mask="url(#mask"#).unwrap();
        let line_start = svg[..at].rfind('<').unwrap();
        assert!(
            !svg[line_start..at].contains("transform="),
            "the wrapper carries no transform: {}",
            &svg[line_start..at]
        );
        assert!(svg.contains("<rect"), "and the artwork is still live");
    }

    #[test]
    fn svg_exports_shapes_rasters_and_text() {
        let mut doc = Document::new(320, 200, ColorMode::Rgb);
        let root = doc.root();

        let mut rect = Node::vector(
            "r",
            VectorShape::Rect {
                width: 40.0,
                height: 30.0,
                radius: 0.0,
            },
        );
        if let NodeKind::Vector { fill, .. } = &mut rect.kind {
            *fill = Some(RED);
        }
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(rect),
        })
        .unwrap();
        let id = doc.children_of(root).unwrap()[0];
        doc.apply(Command::SetTransform {
            id,
            transform: Transform::translation(10.0, 20.0),
        })
        .unwrap();

        let res = doc.add_resource(1, 1, vec![0, 255, 0, 255]);
        doc.apply(Command::AddNode {
            parent: root,
            index: 1,
            node: Box::new(Node::raster(
                "img",
                chitrakar_doc::RasterRef {
                    resource_id: res,
                    width: 1,
                    height: 1,
                },
            )),
        })
        .unwrap();

        doc.apply(Command::AddNode {
            parent: root,
            index: 2,
            node: Box::new(Node::text(
                "t",
                chitrakar_doc::TextSpec::new("a < b", 24.0, RED),
            )),
        })
        .unwrap();

        let svg = export_svg(&doc).unwrap();
        assert!(svg.starts_with("<svg "), "root element");
        assert!(svg.contains(r#"viewBox="0 0 320 200""#));
        assert!(svg.contains(
            r##"<rect width="40" height="30" transform="matrix(1 0 0 1 10 20)" fill="#ff0000""##
        ));
        assert!(
            !svg.contains(" rx="),
            "a square-cornered rect carries no rx"
        );
        assert!(
            svg.contains(r#"href="data:image/png;base64,"#),
            "raster embedded"
        );
        assert!(svg.contains("a &lt; b"), "text XML-escaped");
        // The size is the ascent-to-descent height; the font-size written
        // is the em that scales the face to it, a little smaller.
        let em: f32 = svg
            .split(r#"font-size=""#)
            .nth(1)
            .and_then(|s| s.split('"').next()?.parse().ok())
            .unwrap();
        assert!(em > 19.0 && em < 24.0, "em {em} for a 24px block");

        // A rounded rectangle carries its radius as SVG's own rx.
        doc.apply(Command::AddNode {
            parent: root,
            index: 3,
            node: Box::new(Node::vector(
                "round",
                chitrakar_doc::VectorShape::Rect {
                    width: 40.0,
                    height: 30.0,
                    // Deliberately past half the shorter side, to check the
                    // export clamps it the way the renderer does.
                    radius: 40.0,
                },
            )),
        })
        .unwrap();
        assert!(export_svg(&doc).unwrap().contains(r#" rx="15""#));

        // A second block, with newlines and tracking: each line is its own
        // tspan a line-height down, and the tracking rides on the <text>.
        doc.apply(Command::AddNode {
            parent: root,
            index: 3,
            node: Box::new(Node::text("t2", {
                let mut spec = chitrakar_doc::TextSpec::new("one\ntwo", 20.0, RED);
                spec.line_height = 1.5;
                spec.letter_spacing = 0.1;
                spec
            })),
        })
        .unwrap();
        let svg = export_svg(&doc).unwrap();
        assert!(
            svg.contains(r#"letter-spacing="2.000""#),
            "tracking in ems -> px"
        );
        assert!(svg.contains(">one</tspan>") && svg.contains(">two</tspan>"));
        let baselines: Vec<f32> = svg
            .lines()
            .filter(|l| l.contains("<tspan") && (l.contains(">one<") || l.contains(">two<")))
            .filter_map(|l| {
                let at = l.find(r#"y=""#)? + 3;
                l[at..].split('"').next()?.parse().ok()
            })
            .collect();
        assert_eq!(
            baselines.len(),
            2,
            "two lines, two baselines: {baselines:?}"
        );
        let step = chitrakar_render::text::set(&{
            let mut spec = chitrakar_doc::TextSpec::new("one\ntwo", 20.0, RED);
            spec.line_height = 1.5;
            spec
        })
        .step;
        assert!(
            (baselines[1] - baselines[0] - step).abs() < 0.05,
            "the second line sits the renderer's line step down: {baselines:?} vs {step}"
        );

        // Alignment lands on text-anchor at the x the raster aligns at,
        // and a wrap width folds the words into more tspans.
        doc.apply(Command::AddNode {
            parent: root,
            index: 3,
            node: Box::new(Node::text("t3", {
                let mut spec = chitrakar_doc::TextSpec::new("the quick brown fox", 20.0, RED);
                spec.align = chitrakar_doc::TextAlign::Center;
                spec.width = 90.0;
                spec
            })),
        })
        .unwrap();
        let svg = export_svg(&doc).unwrap();
        let block = svg
            .lines()
            .skip_while(|l| !l.contains(r#"text-anchor="middle""#))
            .take_while(|l| !l.contains("</text>"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            block.contains(r#"<tspan x="45.00""#),
            "centred in the 90px block: {block}"
        );
        let struck = {
            let mut spec = chitrakar_doc::TextSpec::new("x", 20.0, RED);
            spec.underline = true;
            spec.strike = true;
            spec
        };
        doc.apply(Command::AddNode {
            parent: root,
            index: 3,
            node: Box::new(Node::text("t4", struck)),
        })
        .unwrap();
        assert!(
            export_svg(&doc)
                .unwrap()
                .contains(r#" text-decoration="underline line-through""#),
            "decorations ride on the text element"
        );
        // Text along a guide is a textPath over a path in the defs.
        let along = {
            let mut spec = chitrakar_doc::TextSpec::new("round", 20.0, RED);
            spec.along = Some(chitrakar_doc::VectorShape::Ellipse { rx: 40.0, ry: 40.0 });
            spec.along_offset = 12.0;
            spec
        };
        doc.apply(Command::AddNode {
            parent: root,
            index: 3,
            node: Box::new(Node::text("t5", along)),
        })
        .unwrap();
        let svg = export_svg(&doc).unwrap();
        assert!(
            svg.contains(r#"<path id="guide"#) && svg.contains(r#" Z"/>"#),
            "the guide is in the defs"
        );
        assert!(
            svg.contains(r##"<textPath href="#guide"##)
                && svg.contains(r#" startOffset="12.00">round</textPath>"#),
            "{svg}"
        );
        assert!(
            block.matches("<tspan").count() >= 2 && block.contains(">the quick<"),
            "wrapped at the width, as typed: {block}"
        );
    }

    #[test]
    fn bezier_paths_export_as_cubic_curves() {
        // A curve must leave as a curve: C segments, not a polyline of the
        // flattened samples, or every round trip loses the geometry.
        let mut doc = Document::new(100, 100, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(Node::vector(
                "curve",
                chitrakar_doc::VectorShape::Path {
                    points: vec![[0.0, 0.0], [40.0, 0.0]],
                    closed: false,
                    smooth: false,
                    handles: vec![[0.0, 0.0, 10.0, -20.0], [-10.0, -20.0, 0.0, 0.0]],
                    subpaths: Vec::new(),
                },
            )),
        })
        .unwrap();

        let svg = export_svg(&doc).unwrap();
        assert!(
            svg.contains("C10,-20 30,-20 40,0"),
            "cubic segment written from the handles:\n{svg}"
        );
        assert!(!svg.contains(" L"), "not flattened to a polyline:\n{svg}");
    }

    #[test]
    fn gradients_export_as_live_svg_gradients() {
        // Our stops are already in objectBoundingBox units, SVG's default,
        // so a gradient exports as a real <linearGradient> the shape
        // references — not baked into a flat colour or an image.
        let mut doc = Document::new(100, 100, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(Node::vector(
                "g",
                chitrakar_doc::VectorShape::Rect {
                    width: 50.0,
                    height: 50.0,
                    radius: 0.0,
                },
            )),
        })
        .unwrap();
        let id = doc.children_of(root).unwrap()[0];
        doc.apply(Command::SetKind {
            id,
            kind: Box::new(NodeKind::Vector {
                shape: chitrakar_doc::VectorShape::Rect {
                    width: 50.0,
                    height: 50.0,
                    radius: 0.0,
                },
                fill: None,
                stroke: None,
                gradient: Some(chitrakar_doc::Gradient::Linear {
                    from: [0.0, 0.0],
                    to: [1.0, 1.0],
                    stops: vec![
                        chitrakar_doc::GradientStop {
                            offset: 0.0,
                            color: RED,
                        },
                        chitrakar_doc::GradientStop {
                            offset: 1.0,
                            color: AuthoredColor::Srgb {
                                r: 0.0,
                                g: 0.0,
                                b: 1.0,
                                a: 1.0,
                            },
                        },
                    ],
                }),
            }),
        })
        .unwrap();

        let svg = export_svg(&doc).unwrap();
        let name = format!("chitrakar-grad-{}", id.0);
        assert!(svg.contains("<defs>"), "defs block written:\n{svg}");
        assert!(
            svg.contains(&format!(r#"<linearGradient id="{name}""#)),
            "gradient defined:\n{svg}"
        );
        assert!(
            svg.contains(&format!(r##"fill="url(#{name})""##)),
            "shape references it:\n{svg}"
        );
        assert!(
            svg.contains(r##"stop-color="#ff0000""##) && svg.contains(r##"stop-color="#0000ff""##),
            "both stops carried:\n{svg}"
        );
        assert!(
            svg.find("<defs>") < svg.find("<rect"),
            "defs must precede the shape that references them:\n{svg}"
        );
    }

    /// Bold travels as SVG's own word for it, so a reader that has the
    /// family's bold cut sets the block in it and one that does not
    /// thickens the upright — which is what the engine does either way.
    #[test]
    fn bold_travels_as_a_font_weight() {
        let mut doc = Document::new(80, 40, ColorMode::Rgb);
        let root = doc.root();
        let mut node = Node::text("t", chitrakar_doc::TextSpec::new("Hi", 16.0, RED));
        let chitrakar_doc::NodeKind::Text(spec) = &mut node.kind else {
            unreachable!()
        };
        spec.bold = true;
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(node),
        })
        .unwrap();
        let svg = export_svg(&doc).unwrap();
        assert!(svg.contains(r#"font-weight="bold""#), "{svg}");

        let id = doc.children_of(root).unwrap()[0];
        let mut plain = chitrakar_doc::TextSpec::new("Hi", 16.0, RED);
        plain.bold = false;
        doc.apply(Command::SetKind {
            id,
            kind: Box::new(chitrakar_doc::NodeKind::Text(plain)),
        })
        .unwrap();
        assert!(
            !export_svg(&doc).unwrap().contains("font-weight"),
            "and a block that is not bold says nothing about weight"
        );
    }

    /// A block with style runs is cut into tspans where the runs start
    /// and stop, each saying only what its run changes; the text element
    /// around them carries the rest, which is what SVG inheritance is
    /// for. A block with no runs is still one tspan a line.
    #[test]
    fn style_runs_travel_as_tspans_saying_only_what_they_change() {
        let mut doc = Document::new(200, 60, ColorMode::Rgb);
        let root = doc.root();
        let mut node = Node::text("t", chitrakar_doc::TextSpec::new("one two", 16.0, RED));
        let chitrakar_doc::NodeKind::Text(spec) = &mut node.kind else {
            unreachable!()
        };
        let mut run = chitrakar_doc::StyleRun::over(4, 7);
        run.bold = Some(true);
        run.fill = Some(AuthoredColor::Srgb {
            r: 0.0,
            g: 0.0,
            b: 1.0,
            a: 1.0,
        });
        spec.runs = vec![run];
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(node),
        })
        .unwrap();
        let svg = export_svg(&doc).unwrap();

        // Two pieces, the first carrying the position and nothing else.
        assert!(
            svg.contains(r#"<tspan x="0.00" y="#) && svg.contains(">one </tspan>"),
            "the block's own stretch: {svg}"
        );
        assert!(
            svg.contains(r##"<tspan fill="#0000ff" font-weight="bold">two</tspan>"##),
            "and the run's, saying only what it changes: {svg}"
        );
        // Not repeated on the text element, which is the block's own.
        let head = &svg[..svg.find("<tspan").unwrap()];
        assert!(
            head.contains(r##"fill="#ff0000""##) && !head.contains("font-weight"),
            "the block is still the block: {head}"
        );
    }

    #[test]
    fn svg_notes_omitted_layers_and_paths_roundtrip_geometry() {
        let mut doc = Document::new(100, 100, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(Node::adjustment(
                "exp",
                chitrakar_doc::Adjustment::Exposure { stops: 1.0 },
            )),
        })
        .unwrap();
        let mut path = Node::vector(
            "p",
            VectorShape::Path {
                points: vec![[0.0, 0.0], [10.0, 0.0], [5.0, 8.0]],
                closed: true,
                smooth: false,
                handles: Vec::new(),
                subpaths: Vec::new(),
            },
        );
        if let NodeKind::Vector { fill, .. } = &mut path.kind {
            *fill = Some(RED);
        }
        doc.apply(Command::AddNode {
            parent: root,
            index: 1,
            node: Box::new(path),
        })
        .unwrap();

        let svg = export_svg(&doc).unwrap();
        assert!(svg.contains("adjustment layer 'exp' has no SVG equivalent"));
        assert!(
            svg.contains(r#"d="M0,0 L10,0 L5,8 Z""#),
            "path geometry: {svg}"
        );
    }

    const BLUE: AuthoredColor = AuthoredColor::Srgb {
        r: 0.0,
        g: 0.0,
        b: 1.0,
        a: 1.0,
    };

    fn place(doc: &mut Document, node: Node, at: [f32; 2]) -> NodeId {
        let root = doc.root();
        let index = doc.children_of(root).unwrap().len();
        doc.apply(Command::AddNode {
            parent: root,
            index,
            node: Box::new(node),
        })
        .unwrap();
        let id = doc.children_of(root).unwrap()[index];
        doc.apply(Command::SetTransform {
            id,
            transform: Transform {
                e: at[0],
                f: at[1],
                ..Default::default()
            },
        })
        .unwrap();
        id
    }

    fn painted(name: &str, shape: VectorShape, fill: AuthoredColor) -> Node {
        let mut n = Node::vector(name, shape);
        if let NodeKind::Vector { fill: f, .. } = &mut n.kind {
            *f = Some(fill);
        }
        n
    }

    /// A page holding one of everything SVG is meant to carry live: a
    /// plain rect, a rounded one inside a half-opaque group, an ellipse
    /// wearing an inner stroke, a curved compound path with a hole, a
    /// gradient, a placed raster and a line of text.
    fn everything() -> Document {
        let mut doc = Document::new(120, 80, ColorMode::Rgb);
        place(
            &mut doc,
            painted(
                "rect",
                VectorShape::Rect {
                    width: 40.0,
                    height: 30.0,
                    radius: 0.0,
                },
                RED,
            ),
            [10.0, 10.0],
        );

        let mut ring = painted("ring", VectorShape::Ellipse { rx: 15.0, ry: 10.0 }, BLUE);
        if let NodeKind::Vector { stroke, .. } = &mut ring.kind {
            *stroke = Some(chitrakar_doc::Stroke {
                color: RED,
                width: 4.0,
                widths: Vec::new(),
                dash: Vec::new(),
                cap: Default::default(),
                join: Default::default(),
                start_marker: Default::default(),
                end_marker: Default::default(),
                // A band lying outside the edge, which SVG and a page
                // can only say by moving the shape or by clipping — so
                // it is worth a witness watching them say it.
                align: Some(chitrakar_doc::StrokeAlign::Outside),
            });
        }
        place(&mut doc, ring, [60.0, 10.0]);

        place(
            &mut doc,
            painted(
                "curve",
                VectorShape::Path {
                    points: vec![[0.0, 0.0], [30.0, 0.0], [30.0, 30.0], [0.0, 30.0]],
                    closed: true,
                    smooth: false,
                    handles: vec![
                        [0.0, 0.0, 10.0, -8.0],
                        [-10.0, -8.0, 0.0, 0.0],
                        [0.0; 4],
                        [0.0; 4],
                    ],
                    subpaths: vec![vec![[10.0, 10.0], [20.0, 10.0], [20.0, 20.0], [10.0, 20.0]]],
                },
                BLUE,
            ),
            [8.0, 45.0],
        );

        let group = place(&mut doc, Node::group("g"), [50.0, 45.0]);
        doc.apply(Command::SetOpacity {
            id: group,
            opacity: 0.5,
        })
        .unwrap();
        doc.apply(Command::AddNode {
            parent: group,
            index: 0,
            node: Box::new(painted(
                "round",
                VectorShape::Rect {
                    width: 20.0,
                    height: 20.0,
                    radius: 6.0,
                },
                RED,
            )),
        })
        .unwrap();

        let mut ramp = painted(
            "ramp",
            VectorShape::Rect {
                width: 24.0,
                height: 20.0,
                radius: 0.0,
            },
            RED,
        );
        if let NodeKind::Vector { gradient, .. } = &mut ramp.kind {
            *gradient = Some(chitrakar_doc::Gradient::Linear {
                from: [0.0, 0.0],
                to: [1.0, 0.0],
                stops: vec![
                    chitrakar_doc::GradientStop {
                        offset: 0.0,
                        color: RED,
                    },
                    chitrakar_doc::GradientStop {
                        offset: 1.0,
                        color: BLUE,
                    },
                ],
            });
        }
        place(&mut doc, ramp, [80.0, 45.0]);

        let res = doc.add_resource(2, 1, vec![0, 255, 0, 255, 0, 0, 0, 0]);
        let img = place(
            &mut doc,
            Node::raster(
                "img",
                chitrakar_doc::RasterRef {
                    resource_id: res,
                    width: 2,
                    height: 1,
                },
            ),
            [0.0, 0.0],
        );
        doc.apply(Command::SetTransform {
            id: img,
            transform: Transform {
                a: 10.0,
                d: 10.0,
                e: 4.0,
                f: 4.0,
                ..Default::default()
            },
        })
        .unwrap();

        // A line with ends and a corner of its own: squared off past its
        // last point and cut across where it turns, so export has to say
        // both — SVG takes flat ends and mitred corners as read, and a
        // reader drawing those instead would show up here.
        let mut elbow = painted(
            "elbow",
            VectorShape::Path {
                points: vec![[0.0, 0.0], [14.0, 0.0], [14.0, 20.0]],
                closed: false,
                smooth: false,
                handles: Vec::new(),
                subpaths: Vec::new(),
            },
            BLUE,
        );
        if let NodeKind::Vector { fill, stroke, .. } = &mut elbow.kind {
            *fill = None;
            *stroke = Some(chitrakar_doc::Stroke {
                color: BLUE,
                width: 6.0,
                widths: Vec::new(),
                dash: Vec::new(),
                cap: chitrakar_doc::StrokeCap::Square,
                join: chitrakar_doc::StrokeJoin::Bevel,
                // A head at one end and nothing at the other, which is
                // what most arrows are.
                start_marker: chitrakar_doc::Marker::None,
                end_marker: chitrakar_doc::Marker::Arrow,
                align: None,
            });
        }
        place(&mut doc, elbow, [98.0, 6.0]);

        let mut lettering = chitrakar_doc::TextSpec::new("Hi", 16.0, BLUE);
        // The second letter is set apart: another colour, so the page
        // exercises a block that is not all one ink.
        let mut run = chitrakar_doc::StyleRun::over(1, 2);
        run.fill = Some(RED);
        lettering.runs = vec![run];
        place(&mut doc, Node::text("t", lettering), [46.0, 66.0]);
        doc
    }

    /// The last word on export fidelity: an SVG consumer that is not us
    /// draws the page, and it has to come out the page the engine drew.
    /// This is the test that would have caught a stroke written where the
    /// engine does not put it, so it is worth more than any amount of
    /// reading the markup back.
    #[test]
    fn resvg_draws_the_same_page_the_engine_does() {
        let doc = everything();
        let svg = export_svg(&doc).unwrap();

        let mut opt = usvg::Options::default();
        opt.fontdb_mut()
            .load_font_data(include_bytes!("../../render/assets/DejaVuSans.ttf").to_vec());
        opt.font_family = "DejaVu Sans".to_string();
        let tree = usvg::Tree::from_data(svg.as_bytes(), &opt).unwrap();
        assert_eq!((tree.size().width(), tree.size().height()), (120.0, 80.0));

        // Onto paper, so what is compared is what a reader would see.
        let mut drawn = resvg::tiny_skia::Pixmap::new(120, 80).unwrap();
        drawn.fill(resvg::tiny_skia::Color::WHITE);
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::identity(),
            &mut drawn.as_mut(),
        );

        let ours = chitrakar_render::render(&doc).unwrap();
        let mut total = 0u64;
        let mut worst = (0u32, 0usize);
        let mut bad = 0usize;
        for (i, px) in ours.pixels.iter().enumerate() {
            let over = |v: f32| chitrakar_color::linear_to_srgb((v + 1.0 - px.a).clamp(0.0, 1.0));
            let want = [over(px.r), over(px.g), over(px.b)].map(|v| (v * 255.0).round() as i32);
            let got = &drawn.data()[i * 4..i * 4 + 3];
            let mut here = 0u32;
            for (c, w) in want.iter().enumerate() {
                here += (got[c] as i32 - w).unsigned_abs();
            }
            total += here as u64;
            if here > worst.0 {
                worst = (here, i);
            }
            if here > 120 {
                bad += 1;
            }
        }
        let mean = total as f64 / (ours.pixels.len() * 3) as f64;
        // Every diagonal edge on the page costs a little of this, since
        // the two rasterizers antialias one their own way — so the
        // number goes up as the page gains elements, and it is the spot
        // checks below, not this, that catch a shape in the wrong place.
        assert!(
            mean < 3.5,
            "mean channel difference {mean:.2}; worst pixel {},{} off by {}",
            worst.1 % 120,
            worst.1 / 120,
            worst.0
        );
        // What is left over is edges: the glyph outlines each rasterizer
        // antialiases its own way, and the pixels where a half-opaque
        // layer meets paper, which the engine mixes in linear light and
        // every SVG consumer mixes in the encoding a device shows. A
        // shape drawn in the wrong place would put hundreds of pixels
        // here, not dozens, which is what this number is guarding.
        assert!(bad < 200, "{bad} pixels differ badly; worst {}", worst.0);

        // Spot checks, where a mean would hide a shape in the wrong place:
        // inside the rect, the ellipse's band and its middle, the hole in
        // the compound path, the half-opaque group, the image's two pixels.
        let at = |x: usize, y: usize| &drawn.data()[(y * 120 + x) * 4..(y * 120 + x) * 4 + 3];
        assert_eq!(at(30, 25), &[255, 0, 0], "rect");
        assert!(
            at(75, 8)[0] > 200 && at(75, 8)[2] < 60,
            "the ellipse's band lies outside its edge {:?}",
            at(75, 8)
        );
        assert!(
            at(75, 20)[2] > 200 && at(75, 20)[0] < 60,
            "and its middle the fill {:?}",
            at(75, 20)
        );
        assert_eq!(at(23, 60), &[255, 255, 255], "the hole shows paper");
        assert!(
            at(60, 55)[0] > 200 && at(60, 55)[1] > 100,
            "the group is half opaque over paper {:?}",
            at(60, 55)
        );
        assert!(
            at(6, 6)[1] > 200 && at(6, 6)[0] < 60,
            "image pixel {:?}",
            at(6, 6)
        );
        assert!(
            at(18, 6)[0] > 200 && at(18, 6)[2] > 200,
            "and its clear half shows paper, not the pixel beside it {:?}",
            at(18, 6)
        );
        assert!(
            at(112, 28)[2] > 200 && at(112, 28)[0] < 60,
            "the line is squared off past its last point {:?}",
            at(112, 28)
        );
        assert_eq!(at(112, 31), &[255, 255, 255], "and stops there");
        assert_eq!(
            at(114, 3),
            &[255, 255, 255],
            "its corner is cut across, not carried out to a point"
        );
        // The head at the line's far end: inside it, and outside it but
        // within the box it lies in, which is what says a head that had
        // travelled the wrong size or the wrong way round would fail.
        assert!(
            at(108, 12)[2] > 200 && at(108, 12)[0] < 60,
            "the head is drawn where the line points {:?}",
            at(108, 12)
        );
        assert_eq!(
            at(104, 20),
            &[255, 255, 255],
            "and narrows to its tip rather than filling its box"
        );

        // Text is the one thing a mean would forgive being a line off:
        // the glyphs are small and their edges differ anyway. So the ink
        // in the corner the word sits in is boxed in both pictures, and
        // the boxes have to be the same to within a pixel — which is
        // what says the baseline travelled.
        let box_of = |ink: &dyn Fn(usize, usize) -> bool| {
            let (mut lo, mut hi) = ([usize::MAX; 2], [0usize; 2]);
            for y in 66..80 {
                for x in 40..120 {
                    if ink(x, y) {
                        lo = [lo[0].min(x), lo[1].min(y)];
                        hi = [hi[0].max(x), hi[1].max(y)];
                    }
                }
            }
            (lo, hi)
        };
        // The letter set apart by a style run is red on both sides: a
        // mean over a two-letter word would forgive one of them being
        // the wrong colour, and that is the thing being checked.
        let reddest = |ink: &dyn Fn(usize, usize) -> [i32; 3]| {
            (66..80)
                .flat_map(|y| (40..120).map(move |x| (x, y)))
                .filter(|&(x, y)| {
                    let p = ink(x, y);
                    p[0] > 128 && p[2] < 128
                })
                .count()
        };
        let theirs_red = reddest(&|x, y| {
            let p = at(x, y);
            [p[0] as i32, p[1] as i32, p[2] as i32]
        });
        let ours_red = reddest(&|x, y| {
            let px = ours.pixels[y * 120 + x];
            let over = |v: f32| chitrakar_color::linear_to_srgb((v + 1.0 - px.a).clamp(0.0, 1.0));
            [over(px.r), over(px.g), over(px.b)].map(|v| (v * 255.0).round() as i32)
        });
        // How many pixels count is a matter of how each rasterizer
        // antialiases a small letter, so what is held is that both drew
        // the letter red at all — the block's blue would give neither.
        assert!(
            ours_red >= 4 && theirs_red >= 4,
            "the run's letter is red in both: {ours_red} here, {theirs_red} there"
        );

        let theirs = box_of(&|x, y| at(x, y)[0] < 200);
        assert!(
            theirs.1[0] > theirs.0[0],
            "there is a word to box: {theirs:?}"
        );
        let ours_ink = box_of(&|x, y| {
            let px = ours.pixels[y * 120 + x];
            let over = |v: f32| chitrakar_color::linear_to_srgb((v + 1.0 - px.a).clamp(0.0, 1.0));
            over(px.r) * 255.0 < 200.0
        });
        for k in 0..2 {
            assert!(
                theirs.0[k].abs_diff(ours_ink.0[k]) <= 1
                    && theirs.1[k].abs_diff(ours_ink.1[k]) <= 1,
                "the word sits in {ours_ink:?} here and {theirs:?} there"
            );
        }
    }
}
