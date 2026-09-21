//! Adding a layer to a document costs the layer, not the document.
//!
//! Two of the checks `Document::apply` runs after a structural command
//! walked the whole document every time — one for a cycle among copies,
//! one for how deeply the layers nest — so building a document of *n*
//! layers cost *n²*. Measured: 1600 layers took 2.23 seconds to put
//! together, of which 2.22 were those two walks. That is not a slow
//! editor, it is an editor that stops working as a drawing gets big, and
//! it is invisible on the documents a test usually builds.
//!
//! Both checks are still made and both still refuse exactly what they
//! refused. The cycle walk starts from the layer whose arrival made the
//! edge rather than from the root, since a cycle that appeared runs
//! through an edge the command made; and the nesting walk is skipped
//! while the document is known to be shallow enough that one more level
//! cannot reach the limit.
//!
//! This is a test of *shape* rather than of speed, which is why it reads
//! a ratio: four times the layers may cost four times the work and not
//! sixteen. A machine that is slow, or busy, moves both numbers together.

use chitrakar_doc::{Command, Document, Node, NodeKind, Transform, VectorShape};
use std::time::{Duration, Instant};

fn build(n: usize) -> Duration {
    let mut doc = Document::new(400, 300, chitrakar_color::ColorMode::Rgb);
    let root = doc.root();
    let at = Instant::now();
    for i in 0..n {
        let mut node = Node::vector(
            &format!("l{i}"),
            VectorShape::Rect {
                width: 20.0,
                height: 16.0,
                radius: 0.0,
            },
        );
        if let NodeKind::Vector { fill, .. } = &mut node.kind {
            *fill = Some(chitrakar_color::AuthoredColor::Srgb {
                r: (i % 7) as f32 / 7.0,
                g: (i % 5) as f32 / 5.0,
                b: (i % 3) as f32 / 3.0,
                a: 1.0,
            });
        }
        doc.apply(Command::AddNode {
            parent: root,
            index: i,
            node: Box::new(node),
        })
        .unwrap();
        let id = doc.children_of(root).unwrap()[i];
        doc.apply(Command::SetTransform {
            id,
            transform: Transform::translation((i % 19) as f32 * 20.0, (i % 41) as f32 * 7.0),
        })
        .unwrap();
    }
    let spent = at.elapsed();
    assert_eq!(
        doc.children_of(root).unwrap().len(),
        n,
        "the layers are on it"
    );
    spent
}

#[test]
fn adding_a_layer_costs_one_layer() {
    // Warm: the first run of anything pays for pages the allocator has
    // not asked the system for yet.
    let _ = build(200);
    let small = build(400);
    let large = build(1600);
    // Four times the layers. Linear is four times the work; the walks
    // this replaced made it sixteen, and measured they made it more —
    // 17.8ms against 300ms with one of the two still in.
    let ratio = large.as_secs_f64() / small.as_secs_f64().max(1e-9);
    assert!(
        ratio < 6.0,
        "1600 layers cost {large:.2?} against {small:.2?} for 400 — {ratio:.1} times, \
         where four times the layers should be about four times the work"
    );
    // And it measured something: a build that took no time at all would
    // satisfy any ratio, and so would a clock that does not move.
    assert!(
        large > Duration::from_micros(200),
        "the larger build took {large:.2?}, which is too little to have been measured"
    );
}
