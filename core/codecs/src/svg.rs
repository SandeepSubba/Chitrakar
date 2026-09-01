//! SVG export: vector layers export as live SVG elements (the point of a
//! vector editor), rasters embed as data-URI images, text as `<text>`.
//!
//! Faithful subset, honestly scoped: groups carry opacity and blend
//! (`mix-blend-mode`); adjustment and filter layers have no SVG equivalent
//! that matches our compositing and are skipped with a comment; masks are
//! skipped in this first pass.

use chitrakar_color::AuthoredColor;
use chitrakar_doc::{BlendMode, DocError, Document, NodeId, NodeKind, Transform, VectorShape};
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
    write_children(doc, doc.root(), &mut out, 1)?;
    out.push_str("</svg>\n");
    Ok(out)
}

fn write_children(
    doc: &Document,
    group: NodeId,
    out: &mut String,
    depth: usize,
) -> Result<(), DocError> {
    for &child in doc.children_of(group)? {
        let node = doc.node(child)?;
        if !node.visible || node.opacity <= 0.0 {
            continue;
        }
        let pad = "  ".repeat(depth);
        let common = common_attrs(node);
        match &node.kind {
            NodeKind::Group => {
                let _ = writeln!(out, "{pad}<g{common}>");
                write_children(doc, child, out, depth + 1)?;
                let _ = writeln!(out, "{pad}</g>");
            }
            NodeKind::Vector {
                shape,
                fill,
                stroke,
            } => {
                let paint = paint_attrs(doc, fill.as_ref(), stroke.as_ref());
                match shape {
                    VectorShape::Rect { width, height } => {
                        let _ = writeln!(
                            out,
                            r#"{pad}<rect width="{width}" height="{height}"{common}{paint}/>"#
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
                    } => {
                        let mut d = String::new();
                        for (i, p) in points.iter().enumerate() {
                            let _ =
                                write!(d, "{}{},{}", if i == 0 { "M" } else { " L" }, p[0], p[1]);
                        }
                        if *closed {
                            d.push_str(" Z");
                        }
                        let smooth_note = if *smooth {
                            " data-chitrakar-smooth=\"true\""
                        } else {
                            ""
                        };
                        let _ =
                            writeln!(out, r#"{pad}<path d="{d}"{common}{paint}{smooth_note}/>"#);
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
            NodeKind::Text(spec) => {
                let _ = writeln!(
                    out,
                    r#"{pad}<text y="{:.1}" font-family="DejaVu Sans, sans-serif" font-size="{}"{common} fill="{}">{}</text>"#,
                    // Approximate first baseline: SVG text is baseline-
                    // anchored, our node origin is the block's top.
                    spec.size * 0.93,
                    spec.size,
                    color_hex(doc, spec.fill),
                    escape_xml(&spec.text)
                );
            }
            NodeKind::Adjustment(_) | NodeKind::Filter(_) => {
                let _ = writeln!(
                    out,
                    "{pad}<!-- {} layer '{}' has no SVG equivalent; omitted -->",
                    if matches!(node.kind, NodeKind::Adjustment(_)) {
                        "adjustment"
                    } else {
                        "filter"
                    },
                    escape_xml(&node.name)
                );
            }
        }
    }
    Ok(())
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
) -> String {
    let mut s = String::new();
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

    #[test]
    fn svg_exports_shapes_rasters_and_text() {
        let mut doc = Document::new(320, 200, ColorMode::Rgb);
        let root = doc.root();

        let mut rect = Node::vector(
            "r",
            VectorShape::Rect {
                width: 40.0,
                height: 30.0,
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
                chitrakar_doc::TextSpec {
                    text: "a < b".into(),
                    size: 24.0,
                    fill: RED,
                },
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
            svg.contains(r#"href="data:image/png;base64,"#),
            "raster embedded"
        );
        assert!(svg.contains("a &lt; b"), "text XML-escaped");
        assert!(svg.contains(r#"font-size="24""#));
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
