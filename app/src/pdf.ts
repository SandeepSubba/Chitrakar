/** Reading a PDF (or an Illustrator file) — through a module of its own,
 * fetched the first time one is opened rather than with the editor.
 *
 * The PDF renderer is most of a megabyte compressed, which was most of
 * what reading other programs' files added to every visit; few visits
 * open a PDF. The module is `core/pdfin` built by wasm-pack into
 * `wasm-pdf/`, and vite gives the dynamic import its own chunk. */

type PdfModule = typeof import("./wasm-pdf/chitrakar_pdf_in");

let loading: Promise<PdfModule> | null = null;

/** The PDF module, loaded and started once. */
export function pdfReader(): Promise<PdfModule> {
  loading ??= import("./wasm-pdf/chitrakar_pdf_in").then(async (m) => {
    await m.default();
    return m;
  });
  // A failed fetch (offline, before the module was ever kept) is tried
  // again next time rather than remembered.
  loading.catch(() => {
    loading = null;
  });
  return loading;
}

/** A page drawn by the module: its size and RGBA. */
export function unpackPage(out: Uint8Array): { width: number; height: number; rgba: Uint8Array } {
  const view = new DataView(out.buffer, out.byteOffset, 8);
  const [width, height] = [view.getUint32(0, true), view.getUint32(4, true)];
  return { width, height, rgba: out.subarray(8, 8 + width * height * 4) };
}
