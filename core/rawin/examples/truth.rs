//! A photograph with every colour measured at every pixel, for judging a
//! demosaic: each 2×2 block of a Bayer raw (one red, two greens, one
//! blue) becomes one pixel, black off, balanced as shot, white at one.
//! Written as `w h` (u32 LE) then linear RGB f32 LE.
//!
//! `cargo run --release -p chitrakar-raw-in --example truth -- IN.CR2 OUT.bin`
use rawler::rawimage::{RawImageData, RawPhotometricInterpretation};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let bytes = std::fs::read(&a[1]).expect("read");
    let src = rawler::rawsource::RawSource::new_from_slice(&bytes);
    let raw = rawler::decode(&src, &rawler::decoders::RawDecodeParams::default()).expect("decode");
    let RawPhotometricInterpretation::Cfa(c) = &raw.photometric else {
        panic!("not a mosaic")
    };
    assert!(
        c.cfa.is_rgb() && c.cfa.width == 2 && c.cfa.height == 2,
        "not Bayer"
    );
    let v: Vec<f32> = match &raw.data {
        RawImageData::Integer(v) => v.iter().map(|&x| x as f32).collect(),
        RawImageData::Float(v) => v.clone(),
    };
    let black = raw.blacklevel.levels.first().map_or(0.0, |l| l.as_f32());
    let white = *raw.whitelevel.0.first().unwrap_or(&65535) as f32;
    let wb = raw.wb_coeffs;
    let g = wb[1];
    let crop = raw.crop_area.or(raw.active_area).unwrap();
    let (x0, y0) = (crop.p.x & !1, crop.p.y & !1);
    let (w, h) = (crop.d.w / 2, crop.d.h / 2);
    let mut out = Vec::with_capacity(8 + w * h * 12);
    out.extend((w as u32).to_le_bytes());
    out.extend((h as u32).to_le_bytes());
    for by in 0..h {
        for bx in 0..w {
            let mut rgb = [0f32; 3];
            let mut n = [0f32; 3];
            for dy in 0..2 {
                for dx in 0..2 {
                    let (y, x) = (y0 + by * 2 + dy, x0 + bx * 2 + dx);
                    let k = c.cfa.color_at(y, x).min(3);
                    let k = if k == 3 { 1 } else { k };
                    rgb[k] += ((v[y * raw.width + x] - black) / (white - black)).max(0.0);
                    n[k] += 1.0;
                }
            }
            for k in 0..3 {
                let val = (rgb[k] / n[k] * wb[k] / g).min(1.0);
                out.extend(val.to_le_bytes());
            }
        }
    }
    std::fs::write(&a[2], out).expect("write");
    println!("{w}x{h}");
}
