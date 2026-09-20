//! Building a document through a `Session` costs the layers, not the
//! square of them.
//!
//! The document itself was made to stop walking everything after every
//! command; the session in front of it was still doing two walks of its
//! own on top. Both are questions about the whole document that a
//! command can answer by itself — does anything here read a
//! neighbourhood, and is there a copy on the page — and both are now
//! asked of the command rather than of the document, except where a
//! command could have taken the last one away.
//!
//! Measured on the same 1600 layers: 584ms to 47.
//!
//! A test of shape rather than of speed, like its neighbour in
//! `core/doc`: a slow or busy machine moves both numbers together, so
//! what is read is the ratio between them.
//!
//! The ceiling is 12 and not 5, and the gap is a thing still to fix
//! rather than slack. Four times the layers costs about nine times the
//! work here, where linear would be four and the walks this replaced
//! made it sixteen. What is left is `Document::parent_of`, which scans
//! every group's child list to find one layer's parent — so anything
//! that asks what space a layer is in, which the dirty region does
//! twice per command, is a walk of the document wearing a different
//! hat. A parent map fixes it everywhere at once and is its own piece
//! of work; until then this guards against the return of the walks that
//! were taken out, which took the ratio to sixty-four.

use chitrakar_doc::{Command, Node, NodeKind, Transform, VectorShape};
use chitrakar_engine::Session;
use std::time::{Duration, Instant};

fn build(n: usize) -> Duration {
    let mut session = Session::new(400, 300, chitrakar_color::ColorMode::Rgb);
    let root = session.document().root();
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
                r: 0.5,
                g: 0.4,
                b: 0.3,
                a: 1.0,
            });
        }
        session
            .apply(Command::AddNode {
                parent: root,
                index: i,
                node: Box::new(node),
            })
            .unwrap();
        let id = session.document().children_of(root).unwrap()[i];
        session
            .apply(Command::SetTransform {
                id,
                transform: Transform::translation((i % 19) as f32 * 20.0, (i % 41) as f32 * 7.0),
            })
            .unwrap();
    }
    let spent = at.elapsed();
    assert_eq!(
        session.document().children_of(root).unwrap().len(),
        n,
        "the layers are on it"
    );
    spent
}

#[test]
fn a_session_edit_costs_the_edit() {
    let _ = build(200);
    let small = build(400);
    let large = build(1600);
    let ratio = large.as_secs_f64() / small.as_secs_f64().max(1e-9);
    assert!(
        ratio < 12.0,
        "1600 layers cost {large:.2?} against {small:.2?} for 400 — {ratio:.1} times, \
         where the walks this replaced made it sixty-four and what is left \
         to take out makes it nine"
    );
    assert!(
        large > Duration::from_micros(200),
        "the larger build took {large:.2?}, which is too little to have been measured"
    );
}
