fn main() {
    tauri_build::build();
    #[cfg(target_os = "macos")]
    build_subject_matte();
}

/// Compile the Swift that asks the system for a subject's matte.
///
/// Only on Apple platforms, where the model lives. Everywhere else the
/// engine's own colour-based pick is what there is, and the command that
/// calls this says so rather than pretending.
#[cfg(target_os = "macos")]
fn build_subject_matte() {
    use std::process::Command;
    let src = "swift/subject.swift";
    println!("cargo:rerun-if-changed={src}");
    let out = std::env::var("OUT_DIR").expect("OUT_DIR");
    let lib = format!("{out}/libchitrakarsubject.a");
    let status = Command::new("swiftc")
        .args([
            "-emit-library",
            "-static",
            "-O",
            // The Swift runtime is part of the OS on every version this
            // app supports, so it is linked rather than carried.
            "-parse-as-library",
            "-module-name",
            "chitrakarsubject",
            "-o",
            &lib,
            src,
        ])
        .status();
    match status {
        Ok(s) if s.success() => {
            println!("cargo:rustc-link-search=native={out}");
            println!("cargo:rustc-link-lib=static=chitrakarsubject");
            println!("cargo:rustc-link-lib=framework=Vision");
            println!("cargo:rustc-link-lib=framework=CoreVideo");
            println!("cargo:rustc-link-lib=framework=CoreImage");
            println!("cargo:rustc-link-lib=framework=Foundation");
            // Where the OS keeps the Swift runtime the library above
            // expects to find.
            println!("cargo:rustc-link-search=native=/usr/lib/swift");
            println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
            println!("cargo:rustc-cfg=has_subject_matte");
        }
        other => {
            // A machine without a Swift toolchain still builds the app;
            // it just falls back to the engine's own pick, which is what
            // every non-Apple platform does anyway.
            println!(
                "cargo:warning=subject matte unavailable (swiftc: {other:?}); \
                 falling back to the engine's own subject pick"
            );
        }
    }
    println!("cargo:rustc-check-cfg=cfg(has_subject_matte)");
}
