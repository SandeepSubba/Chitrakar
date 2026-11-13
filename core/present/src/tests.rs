use super::*;
use chitrakar_color::{AuthoredColor, ColorMode};
use chitrakar_doc::{Command, Node, VectorShape};
use chitrakar_engine::Session;

/// A presenter, or a note that this machine has no adapter to ask.
fn presenter() -> Option<Presenter> {
    let p = Presenter::new();
    if p.is_none() {
        eprintln!("no graphics adapter here: the presenter's tests ask nothing");
    }
    p
}

fn srgb(r: f32, g: f32, b: f32) -> AuthoredColor {
    AuthoredColor::Srgb { r, g, b, a: 1.0 }
}

/// A page with a picture, a shape and words on it.
fn page() -> (Session, String, Vec<u8>) {
    let mut s = Session::new(120, 90, ColorMode::Rgb);
    let root = s.document().root();
    let add = |s: &mut Session, node: Node| {
        let index = s.document().children_of(root).unwrap().len();
        s.apply(Command::AddNode {
            parent: root,
            index,
            node: Box::new(node),
        })
        .unwrap();
    };
    let mut card = Node::vector(
        "card",
        VectorShape::Rect {
            width: 50.0,
            height: 30.0,
            radius: 6.0,
        },
    );
    card.transform = Transform::translation(60.0, 40.0);
    if let NodeKind::Vector { fill, .. } = &mut card.kind {
        *fill = Some(srgb(0.8, 0.3, 0.2));
    }
    add(&mut s, card);
    add(
        &mut s,
        Node::text(
            "words",
            chitrakar_doc::TextSpec::new("GPU", 18.0, srgb(0.1, 0.1, 0.4)),
        ),
    );
    // Placed last: a picture placed on an empty page is a picture being
    // opened, and takes the page's size with it.
    // A picture: a gradient of its own, so a picture missing or wrong
    // shows.
    let (w, h) = (40u32, 30u32);
    let mut px = Vec::new();
    for y in 0..h {
        for x in 0..w {
            px.extend_from_slice(&[(x * 6) as u8, (y * 8) as u8, 180, 255]);
        }
    }
    let png = chitrakar_codecs::encode_png(w, h, &px).unwrap();
    let id = s.place_image(&png, "picture").unwrap();
    s.apply(Command::SetTransform {
        id,
        transform: Transform::translation(10.0, 12.0),
    })
    .unwrap();
    let picture = s
        .document()
        .resources()
        .next()
        .map(|(id, _)| id.clone())
        .unwrap();
    (s, picture, px)
}

/// What the engine's own frame holds at that view.
fn engine_frame(s: &mut Session, scale: f32, x: f32, y: f32, w: u32, h: u32) -> Vec<u8> {
    s.set_viewport(scale, x, y, w, h);
    s.render_cached().unwrap();
    let mut out = vec![0u8; (w * h * 4) as usize];
    s.encode_present_region(
        chitrakar_engine::ClipRect {
            x0: 0,
            y0: 0,
            x1: w,
            y1: h,
        },
        &mut out,
    );
    out
}

/// How far two frames are apart: the mean over every byte, and how many
/// pixels differ by more than a tenth.
fn apart(a: &[u8], b: &[u8]) -> (f32, usize) {
    let mut sum = 0u64;
    let mut far = 0usize;
    for (p, q) in a.chunks(4).zip(b.chunks(4)) {
        let mut worst = 0u8;
        for c in 0..4 {
            let d = p[c].abs_diff(q[c]);
            sum += d as u64;
            worst = worst.max(d);
        }
        if worst > 25 {
            far += 1;
        }
    }
    (sum as f32 / a.len() as f32, far)
}

#[test]
fn the_mirror_draws_the_frame_the_engine_draws() {
    let Some(mut p) = presenter() else { return };
    let (mut s, picture, px) = page();
    let manifest = serde_json::to_string(s.document()).unwrap();
    let wants = p.sync(&manifest).unwrap();
    assert_eq!(
        wants.resources,
        vec![picture.clone()],
        "the picture is wanted"
    );
    assert!(wants.fonts.is_empty(), "the bundled face is here already");
    assert!(
        p.frame(2.0, 10.0, 6.0, 200, 160).is_none(),
        "and nothing is drawn without it"
    );

    // The wrong bytes are refused, the right ones taken.
    let mut wrong = px.clone();
    wrong[0] ^= 1;
    assert!(p.give_picture(&picture, 40, 30, wrong).is_err());
    assert!(p.give_picture(&picture, 40, 31, px.clone()).is_err());
    p.give_picture(&picture, 40, 30, px.clone()).unwrap();
    assert!(p.wants().nothing());

    // The engine's frame and the mirror's, at the page's own size and at
    // a view that zooms in and looks at part of it.
    for (scale, x, y, w, h) in [(1.0, 0.0, 0.0, 120, 90), (2.5, -40.0, -30.0, 180, 140)] {
        let gpu = p
            .frame(scale, x, y, w, h)
            .expect("the backend draws this page");
        let cpu = engine_frame(&mut s, scale, x, y, w, h);
        assert_eq!(gpu.len(), cpu.len());
        let (mean, far) = apart(&gpu, &cpu);
        assert!(
            mean < 1.0 && far * 50 < (w * h) as usize,
            "at {scale}x: a mean byte apart of {mean}, {far} pixels far apart"
        );
    }

    // Changed and synced again: the change is drawn, and the picture is
    // not asked for twice.
    let card = s.document().children_of(s.document().root()).unwrap()[0];
    let mut kind = s.document().node(card).unwrap().kind.clone();
    if let NodeKind::Vector { fill, .. } = &mut kind {
        *fill = Some(srgb(0.1, 0.8, 0.2));
    }
    s.apply(Command::SetKind {
        id: card,
        kind: Box::new(kind),
    })
    .unwrap();
    let wants = p
        .sync(&serde_json::to_string(s.document()).unwrap())
        .unwrap();
    assert!(wants.nothing(), "{wants:?}");
    let gpu = p.frame(1.0, 0.0, 0.0, 120, 90).unwrap();
    let at = ((55 * 120 + 85) * 4) as usize;
    assert!(
        gpu[at + 1] > 180 && gpu[at] < 100,
        "the card is green now: {:?}",
        &gpu[at..at + 4]
    );
    let cpu = engine_frame(&mut s, 1.0, 0.0, 0.0, 120, 90);
    assert!(apart(&gpu, &cpu).0 < 1.0);
}

#[test]
fn what_the_backend_declines_is_left_to_the_engine() {
    let Some(mut p) = presenter() else { return };
    // A page with a wide median on it: a vote over a window more than
    // eight pixels each way, which the backend hands back.
    let mut s = Session::new(60, 40, ColorMode::Rgb);
    let root = s.document().root();
    s.apply(Command::AddNode {
        parent: root,
        index: 0,
        node: Box::new(Node::filter(
            "quiet",
            chitrakar_doc::Filter::Median { radius: 20.0 },
        )),
    })
    .unwrap();
    assert!(p
        .sync(&serde_json::to_string(s.document()).unwrap())
        .unwrap()
        .nothing());
    assert!(!p.draws(1.0, 0.0, 0.0, 60, 40));
    assert!(p.frame(1.0, 0.0, 0.0, 60, 40).is_none());

    // A CMYK page's colours are its press profile's, which the mirror
    // does not carry.
    let cmyk = Session::new(60, 40, ColorMode::Cmyk);
    p.sync(&serde_json::to_string(cmyk.document()).unwrap())
        .unwrap();
    assert!(p.frame(1.0, 0.0, 0.0, 60, 40).is_none());

    // What is not a document, or not a tree, is refused as opening it is.
    assert!(p.sync("{ not a document").is_err());
    let mut broken: serde_json::Value = serde_json::to_value(s.document()).unwrap();
    broken["children"]["0"] = serde_json::json!([0]);
    assert!(
        p.sync(&broken.to_string()).is_err(),
        "a group holding itself"
    );
}

#[test]
fn a_presenter_can_be_held_by_a_shell() {
    // The shell keeps one behind a lock shared between its commands.
    fn sendable<T: Send>() {}
    sendable::<Presenter>();
}
