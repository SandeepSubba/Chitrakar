//! What a font registry keeps, and what it must not.
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

/// One test rather than two, deliberately: both halves read a counter
/// that belongs to the whole binary, and cargo runs tests in a thread
/// each. Split in two they measured each other — the second's seven
/// megabytes landed inside the first's reading and failed it, which is a
/// flaky test rather than a finding.
#[test]
fn a_font_registry_keeps_what_it_must_and_nothing_else() {
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

    // And the same face offered twice is kept once.
    //
    // Two documents carrying the same file, or somebody loading it
    // twice, should not cost twice. It did: each registration parsed
    // afresh and leaked afresh, and the earlier face — still the same
    // bytes — became unreachable.
    //
    // Replacing a name with *different* bytes still keeps the old face
    // for good, and that is the design rather than an oversight: the
    // registry hands out a `&'static Fonts` and a render in flight may
    // be holding one, so there is no moment the old face could be freed
    // without a borrow outliving it. This covers the case that is an
    // accident, not the one that is a choice.
    let face = include_bytes!("../assets/DejaVuSans.ttf").to_vec();
    let name = "a face offered twice";
    let before = HELD.load(Ordering::Relaxed);
    chitrakar_render::text::register_font(name, face.clone()).expect("the bundled face parses");
    let first = HELD.load(Ordering::Relaxed) - before;
    // Non-vacuity: the first registration really does keep the face, so
    // "nothing more is kept" below is about the repeats and not about a
    // registry that quietly keeps nothing at all.
    assert!(
        first as usize > face.len() / 2,
        "registering a face has to keep it: {first} bytes held for a face \
         of {}",
        face.len()
    );
    let before = HELD.load(Ordering::Relaxed);
    for _ in 0..9 {
        chitrakar_render::text::register_font(name, face.clone()).expect("and again");
    }
    let again = HELD.load(Ordering::Relaxed) - before;
    assert!(
        again < (face.len() / 2) as isize,
        "the same face offered nine more times held {:.2} MiB more. A face \
         already registered under that name, byte for byte, is the face \
         that is wanted — parsing and keeping it again buys nothing and \
         the copy it displaces can never be read.",
        again as f64 / (1 << 20) as f64
    );
}
