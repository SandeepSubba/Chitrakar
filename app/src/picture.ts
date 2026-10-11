/** A picture in a form the engine reads.
 *
 * The engine reads PNG and JPEG itself, with their colour profiles.
 * Anything else a browser can show — WebP, GIF, BMP, AVIF — the browser
 * draws and hands over as a PNG: every webview already carries those
 * decoders, and a WebP decoder in the engine would be six times the
 * weight of the WebP *encoder* it does carry for exporting one.
 */

const isPng = (b: Uint8Array) =>
  b.length > 8 && b[0] === 0x89 && b[1] === 0x50 && b[2] === 0x4e && b[3] === 0x47;
const isJpeg = (b: Uint8Array) => b.length > 3 && b[0] === 0xff && b[1] === 0xd8 && b[2] === 0xff;

/** The bytes as they are when the engine reads them, else a PNG of what
 * the browser draws of them. Decided by what the bytes are rather than
 * what the file is called. A picture the browser cannot draw either
 * comes back as it was, so the engine's own refusal is what is said. */
export async function enginePicture(bytes: Uint8Array, type: string): Promise<Uint8Array> {
  if (isPng(bytes) || isJpeg(bytes)) return bytes;
  try {
    const bitmap = await createImageBitmap(new Blob([bytes as BlobPart], { type }), {
      premultiplyAlpha: "none",
    });
    const canvas = document.createElement("canvas");
    canvas.width = bitmap.width;
    canvas.height = bitmap.height;
    const ctx = canvas.getContext("2d");
    if (!ctx) return bytes;
    ctx.drawImage(bitmap, 0, 0);
    bitmap.close();
    const png = await new Promise<Blob | null>((done) => canvas.toBlob(done, "image/png"));
    return png ? new Uint8Array(await png.arrayBuffer()) : bytes;
  } catch {
    return bytes;
  }
}
