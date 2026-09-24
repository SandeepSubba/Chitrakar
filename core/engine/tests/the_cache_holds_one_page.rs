//! What the render cache keeps.
//!
//! The engine paints what a command touched over what is already on
//! screen, so it keeps the last frame. One frame — and the size of one
//! is why it is worth watching: a surface is premultiplied linear f32,
//! sixteen bytes a pixel, so the 1200 by 900 page here is 16.5 MiB and
//! A4 at 300dpi is 133. A second copy kept by accident is not a
//! rounding error, and "keep the previous frame as well" or "a surface
//! per layer" are both a few lines away.
//!
//! One test in a file of its own: a global allocator belongs to the
//! whole binary and cargo runs tests in a thread each, so two would
//! measure each other.
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

use chitrakar_doc::{Command, Node, Transform, VectorShape};
use chitrakar_engine::Session;

#[test]
fn the_cache_holds_one_page_however_long_it_is_edited() {
    const W: u32 = 1200;
    const H: u32 = 900;
    const SURFACE: usize = W as usize * H as usize * 16;

    let start = HELD.load(Ordering::Relaxed);
    let mut session = Session::new(W, H, chitrakar_color::ColorMode::Rgb);
    let root = session.document().root();
    session
        .apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(Node::vector(
                "r",
                VectorShape::Rect {
                    width: 300.0,
                    height: 200.0,
                    radius: 0.0,
                },
            )),
        })
        .unwrap();
    let id = session.document().children_of(root).unwrap()[0];

    session.render_cached().unwrap();
    let after_one = HELD.load(Ordering::Relaxed) - start;
    // Non-vacuity: a frame really is being kept. Were the cache dropped
    // between calls the ceiling below would hold for the wrong reason.
    assert!(
        after_one as usize > SURFACE / 2,
        "a rendered session holds {after_one} bytes, under half a surface \
         — nothing is being cached, so nothing below is being measured"
    );

    // Rendering again changes nothing: the same frame is repainted in
    // place rather than a new one kept beside it.
    for _ in 0..20 {
        session.render_cached().unwrap();
    }
    // And neither does editing between renders, which is what an editor
    // actually does: fifty commands, each followed by a repaint.
    for i in 0..50 {
        session
            .apply(Command::SetTransform {
                id,
                transform: Transform::translation(i as f32, i as f32),
            })
            .unwrap();
        session.render_cached().unwrap();
    }
    let after_many = HELD.load(Ordering::Relaxed) - start;

    // Half a surface of slack over one, for the document and the
    // history and whatever else a session carries — all of which are
    // kilobytes against sixteen megabytes.
    let ceiling = SURFACE + SURFACE / 2;
    assert!(
        (after_many as usize) < ceiling,
        "after seventy renders and fifty edits the session holds {:.1} MiB, \
         over the {:.1} MiB this allows for a page of {:.1} MiB. The cache \
         is one frame; two is a refactor away and costs 133 MiB a copy on \
         an A4 page at 300dpi.",
        after_many as f64 / (1 << 20) as f64,
        ceiling as f64 / (1 << 20) as f64,
        SURFACE as f64 / (1 << 20) as f64
    );
}
