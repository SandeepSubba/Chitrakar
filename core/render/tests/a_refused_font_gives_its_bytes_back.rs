//! A face that will not parse gives its bytes back.
//!
//! A registered face lives for the whole process: the shaper and the
//! rasterizer both hold borrows of it, so its bytes are leaked on purpose
//! to give them the `'static` they need. The leak used to come *first*,
//! though, with the refusal after it — so a `.chitra` carrying a face
//! that would not parse handed over its bytes and never got them back,
//! and the document opened cleanly with nothing to say so. A file can
//! carry as many faces as it likes.
//!
//! In a test of its own because it needs the whole binary's allocator to
//! answer, and a global allocator is per-binary. Which is also why it is
//! worth having: nothing else in the workspace can see a leak, so this
//! behaviour was unobservable until something was built to watch it.
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};

static HELD: AtomicIsize = AtomicIsize::new(0);

struct Counting;

// Safety: every path forwards to the system allocator unchanged; the
// counter is the only thing added.
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

#[test]
fn a_face_that_will_not_parse_gives_its_bytes_back() {
    const ONE: usize = 1 << 20;
    const TIMES: usize = 20;
    let before = HELD.load(Ordering::Relaxed);
    for i in 0..TIMES {
        let junk: Vec<u8> = (0..ONE).map(|k| ((k + i) % 251) as u8).collect();
        let refused = chitrakar_render::text::register_font(&format!("not a face {i}"), junk);
        // Non-vacuity: if these were somehow accepted the bytes would be
        // held on purpose and the count below would mean nothing.
        assert!(
            refused.is_err(),
            "a megabyte of noise is not a face: {refused:?}"
        );
    }
    let kept = HELD.load(Ordering::Relaxed) - before;
    // A megabyte of slack for whatever else the loop did; the leak this
    // watches for is a megabyte *per face*, so twenty here.
    assert!(
        kept < ONE as isize,
        "{TIMES} MiB of not-a-font was offered and {:.1} MiB is still \
         held. A face that is refused has to give its bytes back — the \
         bytes are leaked on purpose once a face is registered, and that \
         has to happen after the refusal rather than before it.",
        kept as f64 / ONE as f64
    );
}
