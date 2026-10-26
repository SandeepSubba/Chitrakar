// Unit tests for `src/imports.ts` — which road a file takes into the
// editor, by its first bytes and its name. Asked without a browser.
//
//   node e2e/imports.test.mjs
import { build } from "esbuild";
import { unlinkSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const tmp = join(here, "out", "imports.generated.mjs");
await build({
  entryPoints: [join(here, "..", "src", "imports.ts")],
  bundle: true,
  format: "esm",
  outfile: tmp,
  logLevel: "silent",
});
const { importKind, heifBrand, unreadable, PLACE_ACCEPT, RAW_EXTENSIONS } = await import("file://" + tmp);
unlinkSync(tmp);

let ran = 0;
const fails = [];
const check = (cond, msg) => {
  ran += 1;
  if (!cond) fails.push(msg);
};
const bytes = (...parts) =>
  Uint8Array.from(parts.flatMap((p) => (typeof p === "string" ? [...p].map((c) => c.charCodeAt(0)) : p)));

// By what the bytes are.
check(importKind("x", bytes("8BPS", [0, 1])) === "psd", "a Photoshop file by its bytes");
check(importKind("x.bin", bytes("%PDF-1.7\n")) === "pdf", "a PDF by its bytes");
check(importKind("old.ai", bytes("junk\n%PDF-1.5")) === "pdf", "an Illustrator file with its PDF a way in");
check(importKind("x", bytes([0x89], "PNG")) === "picture", "a PNG");
check(importKind("x", bytes([0xff, 0xd8, 0xff, 0xe0])) === "picture", "a JPEG");
check(importKind("scan.tif", bytes("II*", [0])) === "picture", "a TIFF");
check(importKind("x", bytes("<svg xmlns='http://www.w3.org/2000/svg'>")) === "svg", "an SVG");
check(importKind("x", bytes([0, 0, 0, 24], "ftypheic")) === "picture", "a HEIC is a picture");

// A raw file is told by its name, even though it is a TIFF inside.
check(importKind("IMG_0051.CR2", bytes("II*", [0])) === "raw", "a CR2 is a raw file, not a TIFF");
check(importKind("DSC_0059.nef", bytes("MM", [0], "*")) === "raw", "a NEF too");
check(importKind("photo.dng") === "raw", "by name alone");
check(importKind("DSCF0001.RAF", bytes("FUJIFILMCCD-RAW")) === "raw", "a RAF, which is not a TIFF");

// By name, when the bytes are not to hand (a dialog's filter, a path).
check(importKind("a.psd") === "psd" && importKind("big.psb") === "psd", "PSD and PSB by name");
check(importKind("logo.ai") === "pdf" && importKind("doc.PDF") === "pdf", "AI and PDF by name");
check(importKind("photo.heic") === "picture" && importKind("x.avif") === "picture", "HEIC and AVIF by name");
check(importKind("draft.chitra") === "document", "the editor's own document");
check(importKind("3f2a-download", bytes("PK", [3, 4])) === "document", "and one whatever it is called");
check(importKind("notes.txt") === null && importKind("song.mp3", undefined, "audio/mpeg") === null, "nothing else");
check(importKind("unnamed", undefined, "image/png") === "picture", "a picture by its type");

// The dialog offers all of them.
for (const ext of [".psd", ".pdf", ".ai", ".tif", ".heic", ".avif", ".cr2", ".nef", ".dng", ".arw"]) {
  check(PLACE_ACCEPT.split(",").includes(ext), `the Place dialog offers ${ext}`);
}
check(RAW_EXTENSIONS.length >= 30, "the cameras' extensions");

// HEIF brands, and what is said when one cannot be read.
check(heifBrand(bytes([0, 0, 0, 24], "ftypmif1")) === "heic", "mif1 is HEIF");
check(heifBrand(bytes([0, 0, 0, 24], "ftypavif")) === "avif", "avif is AVIF");
check(heifBrand(bytes([0, 0, 0, 24], "ftypisom")) === null, "an MP4 is neither");
const heic = unreadable("IMG_1.HEIC", bytes([0, 0, 0, 24], "ftypheic"), "x");
check(heic.includes("JPEG") && heic.includes("Most Compatible"), `HEIC says how to get it in: ${heic}`);
check(unreadable("a.png", bytes([0x89]), "broken").includes("broken"), "anything else says what went wrong");

if (fails.length) {
  console.error(`${fails.length} of ${ran} failed:\n  ` + fails.join("\n  "));
  process.exit(1);
}
console.log(`imports: ${ran} checks passed`);
