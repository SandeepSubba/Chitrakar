// The subject of a photograph, from the system's own model.
//
// macOS and iOS ship the thing Photos uses to lift a subject off its
// background, and it is better at this than anything this project could
// reasonably carry: it has seen a great many people, which is what it
// takes to know that a white shirt in front of a white curtain is still
// a shirt. Nothing is shipped for it — no weights in the repository, no
// licence to read, no download on first use — because it is already on
// the machine.
//
// Exposed to Rust as two C functions rather than through a binding
// crate: the Swift here is small and readable, and the alternative is
// hand-written Objective-C interop against an API whose shape would have
// to be guessed at.

import Foundation
import Vision
import CoreVideo

/// Work out the subject's matte for a PNG.
///
/// Returns 0 on success, and hands back a buffer of one byte of coverage
/// per pixel that the caller must free with `chitrakar_subject_free`.
/// Non-zero is a failure with nothing allocated: 1 unreadable, 2 the
/// request failed, 3 nothing found in the picture, 4 the mask could not
/// be made.
@_cdecl("chitrakar_subject_matte")
public func chitrakar_subject_matte(
    _ png: UnsafePointer<UInt8>,
    _ pngLen: Int,
    _ outWidth: UnsafeMutablePointer<Int32>,
    _ outHeight: UnsafeMutablePointer<Int32>,
    _ outBytes: UnsafeMutablePointer<UnsafeMutablePointer<UInt8>?>
) -> Int32 {
    let data = Data(bytes: png, count: pngLen)
    let handler = VNImageRequestHandler(data: data, options: [:])
    let request = VNGenerateForegroundInstanceMaskRequest()
    do {
        try handler.perform([request])
    } catch {
        return 2
    }
    guard let found = request.results?.first else { return 3 }
    // Every instance it found, together: a photograph of two people is
    // a photograph of two people, and picking one of them is a
    // different question the caller has not asked.
    let instances = found.allInstances
    if instances.isEmpty { return 3 }
    guard let buffer = try? found.generateScaledMaskForImage(
        forInstances: instances, from: handler
    ) else { return 4 }

    CVPixelBufferLockBaseAddress(buffer, .readOnly)
    defer { CVPixelBufferUnlockBaseAddress(buffer, .readOnly) }
    let w = CVPixelBufferGetWidth(buffer)
    let h = CVPixelBufferGetHeight(buffer)
    guard w > 0, h > 0, let base = CVPixelBufferGetBaseAddress(buffer) else { return 4 }
    let stride = CVPixelBufferGetBytesPerRow(buffer)
    let format = CVPixelBufferGetPixelFormatType(buffer)

    let out = UnsafeMutablePointer<UInt8>.allocate(capacity: w * h)
    if format == kCVPixelFormatType_OneComponent8 {
        for y in 0..<h {
            let row = base.advanced(by: y * stride).assumingMemoryBound(to: UInt8.self)
            out.advanced(by: y * w).update(from: row, count: w)
        }
    } else {
        // The mask comes back as floats on some paths; a byte of
        // coverage is what the caller wants either way.
        for y in 0..<h {
            let row = base.advanced(by: y * stride).assumingMemoryBound(to: Float32.self)
            for x in 0..<w {
                out[y * w + x] = UInt8(max(0, min(255, row[x] * 255.0)))
            }
        }
    }
    outWidth.pointee = Int32(w)
    outHeight.pointee = Int32(h)
    outBytes.pointee = out
    return 0
}

/// Give back what `chitrakar_subject_matte` handed over.
@_cdecl("chitrakar_subject_free")
public func chitrakar_subject_free(_ p: UnsafeMutablePointer<UInt8>?) {
    p?.deallocate()
}
