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
        let node = doc.node(child)?;
        if !node.visible || node.opacity <= 0.0 {
            continue;
        }
        let pad = "  ".repeat(depth);
        let common = common_attrs(node);
        // A mask is authored in the space the layer sits in, so it goes
        // on a wrapper that carries no transform of its own: SVG reads a
        // userSpaceOnUse mask in the space in force where it is
        // referenced, and the layer's own transform belongs inside that.
        let masked = mask_attrs(doc, child, defs);
        if !masked.is_empty() {
            let _ = writeln!(out, "{pad}<g{masked}>");
        }
        match &node.kind {
            NodeKind::Group => {
                let _ = writeln!(out, "{pad}<g{common}>");
                write_children(doc, child, out, depth + 1, defs)?;
                let _ = writeln!(out, "{pad}</g>");
            }
            NodeKind::Vector {
                shape,
                fill,
                stroke,
                gradient,
            } => {
                let paint = paint_attrs(
                    doc,
                    fill.as_ref(),
                    stroke.as_ref(),
                    gradient.as_ref(),
                    child,
                    defs,
                );
                match shape {
                    VectorShape::Rect {
                        width,
                        height,
                        radius,
                    } => {
                        // SVG rounds corners with rx, clamped the same way
                        // the renderer clamps its own radius.
                        let r = radius
                            .max(0.0)
                            .min((width / 2.0).min(height / 2.0).max(0.0));
                        let round = if r > 0.0 {
                            format!(r#" rx="{r}""#)
                        } else {
                            String::new()
                        };
                        let _ = writeln!(
                            out,
                            r#"{pad}<rect width="{width}" height="{height}"{round}{common}{paint}/>"#
                        );
                    }
                    VectorShape::Ellipse { rx, ry } => {
                        let _ = writeln!(
                            out,
                            r#"{pad}<ellipse cx="{rx}" cy="{ry}" rx="{rx}" ry="{ry}"{common}{paint}/>"#
                        );
                    }
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
                    r#"{pad}<text font-family="{}, sans-serif" font-size="{:.2}"{spacing}{style}{decoration}{anchor}{common} fill="{}" xml:space="preserve">"#,
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
                    for (i, (line, _)) in block.lines.iter().enumerate() {
                        let _ = writeln!(
                            out,
                            r#"<tspan x="{:.2}" y="{:.2}">{}</tspan>"#,
                            x,
                            block.ascent + i as f32 * block.step,
                            escape_xml(line)
                        );
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
        BlendMode::Multiply => s.push_str(r#" style="mix-blend-mode:multiply""#),
        BlendMode::Screen => s.push_str(r#" style="mix-blend-mode:screen""#),
    }
    s
}

fn paint_attrs(
    doc: &Document,
    fill: Option<&AuthoredColor>,
    stroke: Option<&chitrakar_doc::Stroke>,
    gradient: Option<&Gradient>,
    id: NodeId,
    defs: &mut String,
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
                    r#" stroke="{}" stroke-width="{}""#,
                    color_hex(doc, stroke.color),
                    stroke.width
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
            r#" stroke="{}" stroke-width="{}""#,
            color_hex(doc, stroke.color),
            stroke.width
        );
    }
    s
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
}
