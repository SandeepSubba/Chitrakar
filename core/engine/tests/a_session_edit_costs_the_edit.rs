//! Adding a layer to a session costs the layer, not the document.
//!
//! The session works out a dirty region for every command, which asks
//! the document what space the layer is drawn in, and the document
//! used to answer that by scanning every group's child list for the
//! layer — so every edit cost the whole document, and four times the
//! layers cost nine times the work. The document keeps a parent map
//! now, and the answer is a lookup: four times the layers costs four
//! times the work, give or take the noise of a timer.
//!
//! Read as a ratio between two sizes rather than as a time, so a slow
//! machine moves both numbers together; the ceiling is six where linear
//! is four, which is margin for a noisy machine and nothing else — the
//! walks that were taken out one by one made it sixty-four, then nine.
//! And a floor on the larger build, since a build too quick to time
//! would satisfy any ratio.

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
        ratio < 6.0,
        "1600 layers cost {large:.2?} against {small:.2?} for 400 — {ratio:.1} times, \
         where linear is four and the walks this replaced made it nine, then \
         sixty-four"
    );
    assert!(
        large > Duration::from_micros(200),
        "the larger build took {large:.2?}, which is too little to have been measured"
    );
}
