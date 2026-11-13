//! What a session keeps as it is edited.
//!
//! Undo here is an inverse command per step, so the history holds the
//! *edit* — and an edit's inverse is about the size of the edit. The
//! other way to build undo, and the more common one, is a copy of the
//! document per step; the two are hard to tell apart by reading and
//! trivial to tell apart by weighing. Two thousand brush strokes cost
//! 1.4 MiB the way this is built and 1120 MiB the other way, which is
//! the whole of why this test exists.
//!
//! It also answers the question the unbounded history invites. Nothing
//! trims `undo`, so a long session grows for ever in principle; in
//! practice a stroke costs about two thirds of a kilobyte, so the
//! growth is the artwork rather than the bookkeeping and a cap would be
//! a product decision rather than a fix.
//!
//! A global allocator belongs to the whole binary and cargo runs tests
//! in a thread each, so this is one test in a file of its own: two of
//! them here would measure each other.
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};

static HELD: AtomicIsize = AtomicIsize::new(0);

struct Counting;

// Safety: every path forwards to the system allocator unchanged.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        HELD.fetch_add(l.size() as isize, Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        HELD.fetch_sub(l.size() as isize, Ordering::Relaxed);
        unsafe { System.dealloc(p, l) }
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

use chitrakar_doc::{Command, Node, PaintStroke};
use chitrakar_engine::Session;

#[test]
fn an_edit_costs_about_what_the_edit_is() {
    const STROKES: usize = 2000;
    const POINTS: usize = 40;
    // What one stroke's own numbers weigh: the points and the radii.
    const PAYLOAD: usize = POINTS * 8 + POINTS * 4;

    let mut session = Session::new(800, 600, chitrakar_color::ColorMode::Rgb);
    let root = session.document().root();
    session
        .apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(Node::paint("brush")),
        })
        .unwrap();
    let layer = session.document().children_of(root).unwrap()[0];

    let before = HELD.load(Ordering::Relaxed);
    for i in 0..STROKES {
        let x = (i % 700) as f32;
        session
            .apply(Command::AddStroke {
                id: layer,
                index: i,
                on_mask: false,
                stroke: Box::new(PaintStroke {
                    points: (0..POINTS)
                        .map(|k| [x + k as f32 * 0.5, 10.0 + k as f32 * 4.0])
                        .collect(),
                    radii: vec![4.0; POINTS],
                    color: chitrakar_color::AuthoredColor::Srgb {
                        r: 0.1,
                        g: 0.2,
                        b: 0.9,
                        a: 1.0,
                    },
                    softness: 0.0,
                    erase: false,
                    source: [0.0, 0.0],
                    heal: false,
                    clip: None,
                }),
            })
            .unwrap();
    }
    let kept = HELD.load(Ordering::Relaxed) - before;

    // Non-vacuity, both ways round. The strokes have to be in the
    // document — a session that refused them would keep nothing and pass
    // — and keeping them has to cost something, or the counter is not
    // watching what it is supposed to.
    let chitrakar_doc::NodeKind::Paint { strokes } = &session.document().node(layer).unwrap().kind
    else {
        panic!("the layer is a paint layer");
    };
    assert_eq!(strokes.len(), STROKES, "every stroke is on the layer");
    assert!(
        kept as usize > STROKES * PAYLOAD / 2,
        "{STROKES} strokes of {PAYLOAD} bytes each are held in under half \
         their own size ({kept} bytes), so this is not measuring them"
    );

    // Five times the strokes' own weight, which is generous for a
    // document and a history that each hold them once over. A history of
    // document copies lands at 1120 MiB here, about eight hundred times
    // this ceiling, so there is no reading of the number that mistakes
    // one for the other.
    let ceiling = STROKES * PAYLOAD * 5;
    assert!(
        (kept as usize) < ceiling,
        "{STROKES} strokes hold {:.1} MiB, over the {:.1} MiB this allows. \
         An edit's inverse is about the size of the edit; a history that \
         holds a copy of the document per step is not, and weighs in at a \
         gigabyte here.",
        kept as f64 / (1 << 20) as f64,
        ceiling as f64 / (1 << 20) as f64
    );

    // And undoing the lot does not grow without bound: the inverses move
    // from one stack to the other rather than being made afresh.
    let before = HELD.load(Ordering::Relaxed);
    let mut undone = 0;
    while session.undo().unwrap() {
        undone += 1;
    }
    assert!(undone >= STROKES, "every stroke undoes: {undone}");
    let moved = HELD.load(Ordering::Relaxed) - before;
    assert!(
        (moved.unsigned_abs()) < STROKES * PAYLOAD,
        "undoing {undone} steps changed what is held by {:.1} MiB, which \
         is more than the edits themselves weigh — an undo hands the \
         inverse to the redo stack rather than building a new one",
        moved as f64 / (1 << 20) as f64
    );
}
