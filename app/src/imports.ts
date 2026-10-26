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
  "erf", "fff", "iiq", "k25", "kdc", "mdc", "mef", "mos", "mrw", "nef", "nrw", "orf", "pef",
  "ptx", "raf", "raw", "rw2", "rwl", "sr2", "srf", "srw", "x3f",
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
  if (ext === "chitra") return "document";
  if (bytes) {
    // A .chitra is a ZIP, and is one whatever it is called — a download
    // is often named by the browser rather than by its maker.
    if (starts(bytes, [0x50, 0x4b, 0x03, 0x04])) return "document";
    if (starts(bytes, ascii("8BPS"))) return "psd";
    // A PDF's header may sit a little way in.
    const head = String.fromCharCode(...bytes.slice(0, 1024));
    if (head.includes("%PDF-")) return "pdf";
    if (RAW_EXTENSIONS.includes(ext)) return "raw";
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
  if (ext === "psd" || ext === "psb") return "psd";
  if (ext === "pdf" || ext === "ai") return "pdf";
  if (ext === "svg" || type === "image/svg+xml") return "svg";
  if (RAW_EXTENSIONS.includes(ext)) return "raw";
  if (PICTURE_EXTENSIONS.includes(ext) || type.startsWith("image/")) return "picture";
  return null;
}

/** The sentence to say when a picture could not be read, rather than a
 * decoder's error: for HEIC, which only a system with its own decoder
 * can read, how to get it in anyway. */
export function unreadable(name: string, bytes: Uint8Array, err: unknown): string {
  if (heifBrand(bytes) === "heic") {
    return (
      `${name} is a HEIC picture, which this system has no decoder for. ` +
      "Export it as JPEG first — Photos does this with File › Export, and an iPhone " +
      "takes JPEGs from the start with Settings › Camera › Formats › Most Compatible."
    );
  }
  if (heifBrand(bytes) === "avif") {
    return `${name} is an AVIF picture, which this browser cannot decode. Open it in a current Chrome, Firefox or Safari, or save it as PNG first.`;
  }
  return `Could not place ${name}: ${err}`;
}
