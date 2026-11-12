/** What a file brought into the editor is, and so which road it takes in.
 *
 * Decided by its first bytes where they say (a PDF, a Photoshop file, a
 * TIFF, a PNG …) and by its name where they do not — a camera's raw file
 * is often a TIFF inside, and read as one gives the small preview at its
 * front, so its extension is what says it is the photograph. One place
 * that knows, read by the file dialog's filter, a drop, a share, the
 * desktop shell's drop and the place step itself. Unit-tested in
 * `e2e/imports.test.mjs`.
 */

export type ImportKind =
  /** The editor's own document: opened, not placed. */
  | "document"
  /** Layers: a Photoshop file (PSD or PSB). */
  | "psd"
  /** A page drawn at a resolution asked for: PDF, or Illustrator. */
  | "pdf"
  /** Shapes: an SVG. */
  | "svg"
  /** A camera's raw file, developed by the engine. */
  | "raw"
  /** A picture: PNG, JPEG, TIFF, WebP, GIF, BMP, AVIF, HEIC … */
  | "picture";

/** The extensions cameras write raw files under — the engine's own list
 * (`core/codecs/src/raw.rs`), which this must keep up with. */
export const RAW_EXTENSIONS = [
  "3fr", "ari", "arw", "bay", "cap", "cr2", "cr3", "crw", "dcr", "dcs", "dng", "drf", "eip",
  "erf", "fff", "gpr", "iiq", "k25", "kdc", "mdc", "mef", "mos", "mrw", "nef", "nrw", "orf", "ori", "pef",
  "ptx", "raf", "raw", "rw2", "rwl", "sr2", "srf", "srw", "sti", "x3f",
];

const PICTURE_EXTENSIONS = [
  "png", "jpg", "jpeg", "jpe", "jfif", "tif", "tiff", "webp", "gif", "bmp", "avif", "heic", "heif",
];

/** Every extension the editor can bring in as layers. */
export const PLACE_EXTENSIONS = ["svg", "psd", "psb", "pdf", "ai", ...PICTURE_EXTENSIONS, ...RAW_EXTENSIONS];

/** What a browser's file dialog offers for Place. */
export const PLACE_ACCEPT = ["image/*", ...PLACE_EXTENSIONS.map((e) => `.${e}`)].join(",");

/** And for Open: the editor's own documents, and anything else it can
 * bring in, which opens as a new document of its own size. */
export const OPEN_ACCEPT = `.chitra,${PLACE_ACCEPT}`;

const extension = (name: string) => /\.([a-z0-9]+)$/i.exec(name)?.[1]?.toLowerCase() ?? "";

const starts = (b: Uint8Array, sig: number[], at = 0) =>
  b.length >= at + sig.length && sig.every((v, i) => b[at + i] === v);
const ascii = (s: string) => [...s].map((c) => c.charCodeAt(0));

/** Whether the bytes are a HEIF picture (an iPhone's HEIC) or an AVIF. */
export function heifBrand(b: Uint8Array): "heic" | "avif" | null {
  if (!starts(b, ascii("ftyp"), 4)) return null;
  const brand = String.fromCharCode(...b.slice(8, 12));
  if (["heic", "heix", "hevc", "hevx", "heim", "heis", "mif1", "msf1"].includes(brand)) return "heic";
  if (["avif", "avis"].includes(brand)) return "avif";
  return null;
}

/** What a file is, from its name and, when they are to hand, its first
 * bytes (a few dozen are enough). `null` for nothing the editor takes. */
export function importKind(name: string, bytes?: Uint8Array, type = ""): ImportKind | null {
  const ext = extension(name);
  // By name alone a .chitra is a document; with its bytes, what they are
  // decides — a PNG named .chitra was refused as a damaged document.
  if (ext === "chitra" && !bytes) return "document";
  if (bytes) {
    // A .chitra is a ZIP, and is one whatever it is called — a download
    // is often named by the browser rather than by its maker.
    if (starts(bytes, [0x50, 0x4b, 0x03, 0x04])) return "document";
    if (starts(bytes, ascii("8BPS"))) return "psd";
    // A PDF's header may sit a little way in.
    const head = String.fromCharCode(...bytes.slice(0, 1024));
    if (head.includes("%PDF-")) return "pdf";
    // A raw file is a TIFF, or a format of the camera maker's own, and
    // never starts as a JPEG or a PNG does: one that does was named for
    // the camera rather than for what it holds (a phone's ".dng" that is
    // its JPEG), and is a picture.
    const jpegOrPng = starts(bytes, [0xff, 0xd8, 0xff]) || starts(bytes, [0x89, 0x50, 0x4e, 0x47]);
    if (RAW_EXTENSIONS.includes(ext) && !jpegOrPng) return "raw";
    if (
      starts(bytes, [0x89, 0x50, 0x4e, 0x47]) ||
      starts(bytes, [0xff, 0xd8, 0xff]) ||
      starts(bytes, ascii("II*\0")) ||
      starts(bytes, ascii("MM\0*")) ||
      starts(bytes, ascii("GIF8")) ||
      starts(bytes, ascii("BM")) ||
      (starts(bytes, ascii("RIFF")) && starts(bytes, ascii("WEBP"), 8)) ||
      heifBrand(bytes)
    )
      return "picture";
    if (/^\s*(<\?xml|<svg|<!--)/i.test(head) && /<svg[\s>]/i.test(head)) return "svg";
  }
  // Bytes that are none of those, under the document's name: a damaged
  // document, and said to be one.
  if (ext === "chitra") return "document";
  if (ext === "psd" || ext === "psb") return "psd";
  if (ext === "pdf" || ext === "ai") return "pdf";
  if (ext === "svg" || type === "image/svg+xml") return "svg";
  if (RAW_EXTENSIONS.includes(ext)) return "raw";
  if (PICTURE_EXTENSIONS.includes(ext) || type.startsWith("image/")) return "picture";
  return null;
}

/** What an engine's refusal says, in words a person reads: without the
 * "Error: invalid command:" every refusal came wrapped in, and with a
 * decoder's own words (a CRC in hex, a serde field and column, "out of
 * memory") said as what they mean for the file. A sentence the engine
 * already wrote for a person comes through as it was. */
export function said(err: unknown): string {
  const text = String((err as { message?: unknown } | null)?.message ?? err)
    .trim()
    .replace(/^(?:Error|RuntimeError|JsError):\s*/i, "")
    .replace(/^invalid command:\s*/i, "")
    .replace(/^(?:Error|io error):\s*/i, "")
    // A library's own name for what went wrong, in brackets after the
    // sentence that already says it.
    .replace(/\s*\((?:InvalidProfile|UnsupportedProfileConnection|InvalidFont)\)/g, "");
  const plain: [RegExp, string][] = [
    [/out of memory|memory limit exceeded|capacity overflow|allocation/i, "it is larger than the editor can hold in memory"],
    [
      // The last, the browser's words for a reader that ran off its own
      // memory: a raw whose directories point back at themselves.
      /crc error|format error decoding|unexpected ?eof|unexpected end|failed to fill whole buffer|invalid (?:png|jpeg|signature)|corrupt|outside the bounds of the DataView/i,
      "the file is damaged or cut short",
    ],
    [/manifest error|missing field|invalid type: .* expected/i, "the document inside the file is damaged, or from a version that wrote it differently"],
    // Before "not well-formed": a drawing whose uses of one another fan
    // out past the reader's count is whole, only too big to read.
    // usvg's words for a drawing saved as UTF-16, ahead of the rest.
    [/has not an? UTF-8 encoding|not valid utf-8|invalid utf-8/i, "the SVG is not written in UTF-8 (it may be saved as UTF-16, or in an older encoding such as Latin-1) — save it again as UTF-8 first"],
    [/nodes limit reached|elements limit/i, "the drawing has more parts than can be read (its uses of one another multiply too far)"],
    [/svg data parsing failed|xml|unexpected .* tag|expected '.*' tag/i, "the SVG is not well-formed (an unclosed or mismatched tag)"],
    [/invalid zip|zip archive|could not find central directory|invalid checksum/i, "the file is damaged: it is not a whole .chitra"],
    [/image format \w+ is not supported/i, "the picture could not be decoded here (it may be damaged)"],
    // A TIFF's reader on a kind it does not take: Lab, or more channels
    // than colour and one transparency (a saved selection beside them).
    [
      /does not support the image format `CIELab/i,
      "it is a Lab TIFF, which the editor does not read — save it as RGB first",
    ],
    [
      /does not support the image format `\w+ with \[[\d, ]+\] bits per sample/i,
      "the TIFF holds channels in a way the editor does not read (CMYK with transparency, or extra channels such as a saved selection) — save it as RGB, or without the extra channels, first",
    ],
    [/image format could not be determined/i, "it is not a picture this editor can read"],
    [/^InvalidFont$/i, "it is not a font file this editor can read"],
    // A colour profile's reader's own words.
    [/^InvalidProfile$/i, "the file is damaged, or is not a colour profile"],
    [/profile device space is Gray, expected RGB/i, "it is a grey profile, and a screen's is a colour (RGB) one"],
    [/profile device space is \w+, expected RGB/i, "it is not a screen's colour (RGB) profile"],
    [/profile device space is \w+, expected CMYK/i, "it is not a press (CMYK) profile"],
    [/specified file not found in archive/i, "the file is damaged: it is not a whole .chitra"],
    [
      /failed to fetch dynamically imported module|importing a module script failed|error loading dynamically imported module/i,
      // Reload, not just try again: a browser keeps a module that failed
      // to load as failed for as long as the page is open.
      "the part of the editor that reads it has not been downloaded yet — connect to the internet and reload the editor, then try again",
    ],
    // Any other download the browser could not make: after the one above,
    // which is the same words about a part of the editor.
    [/failed to fetch|networkerror|load failed|network request failed/i, "the connection failed — check it and try again"],
  ];
  for (const [re, says] of plain) if (re.test(text)) return says;
  return text;
}

/** The sentence to say when a picture could not be read, rather than a
 * decoder's error: for HEIC, which only a system with its own decoder
 * can read, how to get it in anyway. */
export function unreadable(name: string, bytes: Uint8Array, err: unknown, verb = "place"): string {
  if (heifBrand(bytes) === "heic") {
    return (
      `${name} is a HEIC picture, which this system has no decoder for. ` +
      "Export it as JPEG first — Photos does this with File › Export, and an iPhone " +
      "takes JPEGs from the start with Settings › Camera › Formats › Most Compatible."
    );
  }
  if (heifBrand(bytes) === "avif") {
    // Which of the two it is the browser does not say: a current one
    // reads AVIF, and one the file is cut short in says the same as one
    // with no decoder at all.
    return `${name} is an AVIF picture this browser could not decode: the file may be damaged, or the browser too old to read AVIF (a current Chrome, Firefox or Safari reads it). Save it as PNG first if it opens elsewhere.`;
  }
  return `Could not ${verb} ${name}: ${said(err)}`;
}
