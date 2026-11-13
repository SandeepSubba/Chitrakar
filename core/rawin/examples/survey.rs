//! Read, develop and time every raw file in a folder; write a contact
//! sheet. `cargo run --release -p chitrakar-raw-in --example survey -- DIR OUT.png`
//! (`TILE=500 COLS=3` for bigger thumbnails). The samples at raw.pixls.us
//! are what it was written for, and are not committed.
use chitrakar_doc::RawDevelop;
use std::time::Instant;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (dir, out) = (&a[1], &a[2]);
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    files.sort();
    std::panic::set_hook(Box::new(|_| {}));

    let tile: usize = std::env::var("TILE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200);
    let mut thumbs: Vec<(String, Option<Vec<u8>>)> = Vec::new();
    println!(
        "{:<46} {:>9} {:>11} {:>7} {:>6} {:>8}  result",
        "file", "read", "size", "kelvin", "tint", "develop"
    );
    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let bytes = std::fs::read(&path).unwrap();
        let t = Instant::now();
        let got = std::panic::catch_unwind(|| chitrakar_raw_in::read(&bytes));
        let read = t.elapsed();
        let packed = match got {
            Err(_) => {
                println!(
                    "{name:<46} {read:>9.2?} {:>11} {:>7} {:>6} {:>8}  PANICKED in the reader",
                    "", "", "", ""
                );
                thumbs.push((name, None));
                continue;
            }
            Ok(Err(e)) => {
                println!(
                    "{name:<46} {read:>9.2?} {:>11} {:>7} {:>6} {:>8}  refused: {}",
                    "",
                    "",
                    "",
                    "",
                    e.chars().take(90).collect::<String>()
                );
                thumbs.push((name, None));
                continue;
            }
            Ok(Ok(p)) => p,
        };
        let photo = match chitrakar_codecs::raw::unpack(&packed) {
            Ok(p) => p,
            Err(e) => {
                println!("{name:<46} {read:>9.2?}  unpack failed: {e}");
                thumbs.push((name, None));
                continue;
            }
        };
        let (w, h) = photo.size();
        let [k, tint] = photo.as_shot_white();
        let t = Instant::now();
        let full = std::panic::catch_unwind(|| photo.develop(&RawDevelop::default()));
        let dev = t.elapsed();
        let Ok(full) = full else {
            println!("{name:<46} {read:>9.2?} {:>11} {k:>7} {tint:>6.0} {dev:>8.2?}  PANICKED in develop", format!("{w}x{h}"));
            thumbs.push((name, None));
            continue;
        };
        // A thumbnail, fitted into tile x tile, from the full develop.
        let s = (full.width.max(full.height) as f32 / tile as f32).max(1.0);
        let mut th = vec![40u8; tile * tile * 4];
        let (tw, thh) = (
            (full.width as f32 / s) as usize,
            (full.height as f32 / s) as usize,
        );
        let (ox, oy) = ((tile - tw) / 2, (tile - thh) / 2);
        for y in 0..thh {
            for x in 0..tw {
                let sx = ((x as f32 + 0.5) * s) as usize;
                let sy = ((y as f32 + 0.5) * s) as usize;
                let i = (sy.min(full.height as usize - 1) * full.width as usize
                    + sx.min(full.width as usize - 1))
                    * 4;
                let o = ((oy + y) * tile + ox + x) * 4;
                th[o..o + 4].copy_from_slice(&full.rgba8[i..i + 4]);
            }
        }
        let mean: f32 = full
            .rgba8
            .chunks_exact(4)
            .step_by(97)
            .map(|p| (p[0] as f32 + p[1] as f32 + p[2] as f32) / 3.0)
            .sum::<f32>()
            / (full.rgba8.len() / 4 / 97) as f32;
        let warn = if !(2500.0..12000.0).contains(&k) {
            "  kelvin odd"
        } else {
            ""
        };
        println!("{name:<46} {read:>9.2?} {:>11} {k:>7} {tint:>6.0} {dev:>8.2?}  ok, mean {mean:.0}{warn}", format!("{w}x{h}"));
        thumbs.push((name, Some(th)));
    }
    // The sheet: eight across.
    let cols: usize = std::env::var("COLS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8);
    let rows = thumbs.len().div_ceil(cols);
    let (sw, sh) = (cols * tile, rows * tile);
    let mut sheet = vec![20u8; sw * sh * 4];
    for (i, (_, th)) in thumbs.iter().enumerate() {
        let (cx, cy) = ((i % cols) * tile, (i / cols) * tile);
        match th {
            Some(th) => {
                for y in 0..tile {
                    let o = ((cy + y) * sw + cx) * 4;
                    sheet[o..o + tile * 4].copy_from_slice(&th[y * tile * 4..(y + 1) * tile * 4]);
                }
            }
            None => {
                for y in 0..tile {
                    for x in 0..tile {
                        let o = ((cy + y) * sw + cx + x) * 4;
                        sheet[o..o + 4].copy_from_slice(&[120, 0, 0, 255]);
                    }
                }
            }
        }
    }
    std::fs::write(
        out,
        chitrakar_codecs::encode_png(sw as u32, sh as u32, &sheet).unwrap(),
    )
    .unwrap();
}
