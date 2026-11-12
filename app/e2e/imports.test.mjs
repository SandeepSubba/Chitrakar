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
const { importKind, heifBrand, unreadable, said, PLACE_ACCEPT, RAW_EXTENSIONS } = await import("file://" + tmp);
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
check(importKind("20190401_120000.dng", bytes([0xff, 0xd8, 0xff, 0xe1])) === "picture", "a phone's .dng that is its JPEG is a picture");
check(importKind("odd.cr2", bytes([0x89], "PNG")) === "picture", "and a .cr2 that is a PNG");

// By name, when the bytes are not to hand (a dialog's filter, a path).
check(importKind("a.psd") === "psd" && importKind("big.psb") === "psd", "PSD and PSB by name");
check(importKind("logo.ai") === "pdf" && importKind("doc.PDF") === "pdf", "AI and PDF by name");
check(importKind("photo.heic") === "picture" && importKind("x.avif") === "picture", "HEIC and AVIF by name");
check(importKind("draft.chitra") === "document", "the editor's own document");
check(importKind("3f2a-download", bytes("PK", [3, 4])) === "document", "and one whatever it is called");
check(importKind("odd.chitra", bytes([0x89], "PNG")) === "picture", "a PNG named .chitra is the PNG");
check(importKind("broken.chitra", bytes("junk")) === "document", "and junk named .chitra a damaged document");
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

// An engine's refusal, said in words.
check(said(new Error("invalid command: the picture is 9 × 9")) === "the picture is 9 × 9", "the wrapping comes off a sentence");
check(said("Error: invalid command: io error: out of memory").includes("larger than"), "out of memory is said as size");
check(
  said("invalid command: SVG data parsing failed cause nodes limit reached").includes("more parts"),
  "an SVG over the reader's count is too big, not broken",
);
check(said("InvalidProfile").includes("damaged"), "a profile's reader's own word is said in words");
check(said("provided data has not an UTF-8 encoding").includes("UTF-16"), "an SVG in UTF-16 is said in words");
check(said("camera raw: Offset is outside the bounds of the DataView").includes("damaged"), "a raw that loops on itself is said as damaged");
check(said("camera raw: Error: No decoder found, model '', make: '', mode: ''").includes("does not know this camera"), "an unknown camera is said in words");
check(said("subview_until_eof(): Offset 354+4096 is behind EOF").includes("damaged"), "a raw cut short is said as damaged");
check(said("profile device space is Gray, expected RGB").includes("grey profile"), "a grey profile for a screen is said");
check(said(new TypeError("Failed to fetch")).includes("connection"), "a fetch that failed is said as the connection");
check(
  said(new TypeError("Failed to fetch dynamically imported module: http://x/assets/chitrakar_pdf_in-1.js")).includes("not been downloaded"),
  "a reader not fetched while offline is said in words",
);
check(
  said("Error: invalid command: Format error decoding Png: CRC error: expected 0xabd4fb1c").includes("damaged"),
  "a decoder's CRC is a damaged file",
);
check(said("manifest error: missing field `meta` at line 1 column 3091").includes("document inside"), "a manifest's field");
check(said("Invalid checksum").includes("not a whole .chitra"), "a zip's checksum is a damaged document");
check(said("SVG data parsing failed cause expected 'g' tag, not 'svg' at 1:152").includes("not well-formed"), "an SVG's XML");
check(said("The image format Bmp is not supported").includes("could not be decoded"), "a format the engine leaves to the browser");
check(
  said("The Decoder does not support the image format `RGB with [8, 8, 8, 8, 8] bits per sample`").includes("extra channels"),
  "a TIFF with a selection beside its alpha is said in words",
);
check(said("The Decoder does not support the image format `CIELab with [8, 8, 8] bits per sample`").includes("Lab"), "a Lab TIFF too");

if (fails.length) {
  console.error(`${fails.length} of ${ran} failed:\n  ` + fails.join("\n  "));
  process.exit(1);
}
console.log(`imports: ${ran} checks passed`);
