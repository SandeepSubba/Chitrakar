//! Measure each raw file's colour fringes (`RawPhoto::fringes`) and,
//! at the spots `SPOTS` names (fractions across and down, `x,y;x,y`),
//! write the photograph at its own size three ways side by side and
//! enlarged: no corrections, the file's own, and the fringes measured.
//! `ONLY=1` measures and writes nothing. A thumbnail goes beside them,
//! to find the edges by.
//! `cargo run --release -p chitrakar-raw-in --example fringes -- FILE... OUTDIR`
use chitrakar_doc::RawDevelop;
use std::time::Instant;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (files, out) = a.split_at(a.len() - 1);
    for path in files {
        let bytes = std::fs::read(path).unwrap();
        let packed = chitrakar_raw_in::read(&bytes).unwrap();
        let photo = chitrakar_codecs::raw::unpack(&packed).unwrap();
        let (w, h) = photo.size();
        let t = Instant::now();
        let f = photo.fringes();
        let half = ((w * w + h * h) as f32).sqrt() * 0.5;
        println!(
            "{path}: {w}x{h} has lens {} measured in {:.2?}: {f:?} most {:.2} px",
            photo.has_lens(),
            t.elapsed(),
            f.map_or(0.0, |f| f.most(half))
        );
        if std::env::var("ONLY").is_ok() {
            continue;
        }
        let name = std::path::Path::new(path)
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let th = photo.preview(600).develop(
            &photo,
            &RawDevelop {
                curve: true,
                uncorrected: true,
                ..Default::default()
            },
        );
        std::fs::write(
            format!("{}/{name}-thumb.png", out[0]),
            chitrakar_codecs::encode_png(th.width, th.height, &th.rgba8).unwrap(),
        )
        .unwrap();
        let variants = [
            RawDevelop {
                uncorrected: true,
                curve: true,
                ..Default::default()
            },
            RawDevelop {
                curve: true,
                ..Default::default()
            },
            RawDevelop {
                uncorrected: true,
                fringe: true,
                curve: true,
                ..Default::default()
            },
        ];
        let spots: Vec<(f32, f32)> = std::env::var("SPOTS")
            .unwrap_or("0.1,0.1".into())
            .split(';')
            .map(|p| {
                let v: Vec<f32> = p.split(',').map(|x| x.parse().unwrap()).collect();
                (v[0], v[1])
            })
            .collect();
        const SIZE: usize = 120;
        const ZOOM: usize = 3;
        for (k, &(fx, fy)) in spots.iter().enumerate() {
            let parts: Vec<Vec<u8>> = variants
                .iter()
                .map(|s| photo.develop_square(s, fx, fy, SIZE as u32).rgba8)
                .collect();
            let (ow, oh) = (
                SIZE * ZOOM * parts.len() + 8 * (parts.len() - 1),
                SIZE * ZOOM,
            );
            let mut img = vec![255u8; ow * oh * 4];
            for (i, part) in parts.iter().enumerate() {
                for y in 0..oh {
                    for x in 0..SIZE * ZOOM {
                        let from = ((y / ZOOM) * SIZE + x / ZOOM) * 4;
                        let to = (y * ow + i * (SIZE * ZOOM + 8) + x) * 4;
                        img[to..to + 4].copy_from_slice(&part[from..from + 4]);
                    }
                }
            }
            let png = chitrakar_codecs::encode_png(ow as u32, oh as u32, &img).unwrap();
            std::fs::write(format!("{}/{name}-{k}.png", out[0]), png).unwrap();
        }
    }
}
