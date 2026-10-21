//! How much memory reading a photograph and saving a document take,
//! counted by the allocator itself. Its own test binary, so nothing else
//! allocates while it counts.
//!
//! In the browser an engine's memory, once grown, is never handed back,
//! so a peak is not a moment: it is what the tab weighs for the rest of
//! the visit. Two peaks used to be far above what was kept — a picture
//! decoded as RGB and then copied into RGBA, both alive at once; and a
//! save that cloned the whole document, every picture's pixels with it,
//! to write a manifest that never writes pixels. On a 12-megapixel
//! photograph those were 36 MB and 48 MB, and the recover draft saves on
//! its own every little while.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

struct Counting;
static NOW: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let n = NOW.fetch_add(l.size(), Relaxed) + l.size();
        PEAK.fetch_max(n, Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        NOW.fetch_sub(l.size(), Relaxed);
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        if n > l.size() {
            let now = NOW.fetch_add(n - l.size(), Relaxed) + n - l.size();
            PEAK.fetch_max(now, Relaxed);
        } else {
            NOW.fetch_sub(l.size() - n, Relaxed);
        }
        unsafe { System.realloc(p, l, n) }
    }
}

#[global_allocator]
static A: Counting = Counting;

/// How far above what was alive when it began `f` reached.
fn above<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let base = NOW.load(Relaxed);
    PEAK.store(base, Relaxed);
    let out = f();
    (out, PEAK.load(Relaxed).saturating_sub(base))
}

#[test]
fn reading_and_saving_a_photograph_hold_one_copy_of_it() {
    let (w, h) = (1600u32, 1200u32);
    let rgba = (w * h * 4) as usize;
    // A photograph's kind of picture: colour with no alpha, which is what
    // was decoded and then copied.
    // Smooth, as most of a photograph is, so the file is small beside
    // the pixels and a copy of the pixels would stand out.
    let rgb: Vec<u8> = (0..(w * h) as usize)
        .flat_map(|p| {
            let (x, y) = (p as u32 % w, p as u32 / w);
            [(x * 255 / w) as u8, (y * 255 / h) as u8, 128]
        })
        .collect();
    let mut png = Vec::new();
    image::ImageEncoder::write_image(
        image::codecs::png::PngEncoder::new(&mut png),
        &rgb,
        w,
        h,
        image::ExtendedColorType::Rgb8,
    )
    .unwrap();
    drop(rgb);

    let (img, peak) = above(|| chitrakar_codecs::decode(&png).unwrap());
    assert!(
        peak < rgba + rgba / 4,
        "reading it held {} MB for a {} MB picture",
        peak / 1_000_000,
        rgba / 1_000_000
    );

    let mut doc = chitrakar_doc::Document::new(w, h, chitrakar_color::ColorMode::Rgb);
    let id = doc.add_resource(img.width, img.height, img.rgba8);
    let root = doc.root();
    doc.apply(chitrakar_doc::Command::AddNode {
        parent: root,
        index: 0,
        node: Box::new(chitrakar_doc::Node::raster(
            "photo",
            chitrakar_doc::RasterRef {
                resource_id: id,
                width: w,
                height: h,
                corners: None,
            },
        )),
    })
    .unwrap();
    let (bytes, peak) = above(|| chitrakar_codecs::save_chitra(&doc).unwrap());
    assert!(!bytes.is_empty());
    // What a save rightly holds is the file it is writing and the
    // encoder's own buffers: well under the picture itself. A second copy
    // of the pixels would be all of it again.
    assert!(
        peak < rgba / 2 + bytes.len() * 3,
        "saving held {} MB for a {} MB picture",
        peak / 1_000_000,
        rgba / 1_000_000
    );
}
