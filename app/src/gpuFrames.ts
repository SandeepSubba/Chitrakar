/** Frames drawn by the desktop shell's graphics card.
 *
 * The engine in this page draws every frame on the processor. In the
 * desktop shell there is a graphics card beside it, and the shell keeps
 * a mirror of the document to draw there (`core/present`, reached
 * through `shells/tauri/src-tauri/src/gpu.rs`). This is the page's half:
 * it hands the mirror the document whenever the document has changed —
 * its JSON, then the pictures and faces the mirror says it lacks, each
 * once — and asks for the frame the engine would have drawn.
 *
 * The shell answers with the frame's bytes, or with nothing when the
 * card cannot draw that frame exactly as the processor would; then, and
 * on any failure at all, the page draws the frame itself as it always
 * has. So turning this on can only change how fast the page is drawn.
 */

import type { WasmSession } from "./engine";

/** What the engine is looking at: its own viewport arguments. */
export type View = { scale: number; x: number; y: number; width: number; height: number };

type Wants = { resources: string[]; fonts: string[] };

async function call<T>(
  cmd: string,
  args?: Record<string, unknown> | Uint8Array,
  options?: { headers: Record<string, string> },
): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args, options);
}

export class GpuFrames {
  /** What the adapter calls itself; `null` once asked and refused, and
   * `undefined` until then. */
  private adapter: string | null | undefined = undefined;
  /** The JSON the mirror last took. */
  private sent = "";
  private busy = false;
  /** A frame asked for while one was being drawn: drawn after it, once,
   * from the session as it then stands. */
  private pending: (() => void) | null = null;

  constructor(
    /** Lay a whole frame of straight sRGB bytes on the canvas. */
    private readonly paint: (bytes: Uint8ClampedArray, width: number, height: number) => void,
  ) {}

  /** Whether there is a card to draw with, asking the shell once. */
  async ready(): Promise<boolean> {
    if (this.adapter === undefined) {
      try {
        this.adapter = await call<string | null>("gpu_start");
      } catch {
        this.adapter = null;
      }
    }
    return this.adapter !== null;
  }

  name(): string | null {
    return this.adapter ?? null;
  }

  /** Draw a frame of the session as it stands, or call `fallback` to
   * have the page draw it. */
  request(session: WasmSession, view: () => View, fallback: () => void): void {
    const go = () => {
      void this.frame(session, view(), fallback);
    };
    if (this.busy) {
      this.pending = go;
      return;
    }
    go();
  }

  private async frame(session: WasmSession, view: View, fallback: () => void) {
    this.busy = true;
    try {
      if (!(await this.ready()) || !session.frame_is_plain_srgb()) {
        fallback();
        return;
      }
      const json = session.document_json();
      if (json !== this.sent) {
        const wants = await call<Wants>("gpu_sync", { manifest: json });
        for (const id of wants.resources) {
          const size = session.resource_size(id);
          if (size.length !== 2) continue;
          await call<void>("gpu_picture", session.resource_rgba(id), {
            headers: { id, width: String(size[0]), height: String(size[1]) },
          });
        }
        for (const name of wants.fonts) {
          const bytes = session.font_file(name);
          if (bytes.length === 0) continue;
          await call<void>("gpu_font", bytes, { headers: { name: encodeURIComponent(name) } });
        }
        this.sent = json;
      }
      const reply = await call<ArrayBuffer>("gpu_frame", { ...view });
      const bytes = new Uint8ClampedArray(reply);
      if (bytes.length === view.width * view.height * 4) {
        this.paint(bytes, view.width, view.height);
      } else {
        fallback();
      }
    } catch (err) {
      // The page can always draw for itself; a shell that cannot is
      // asked again next time with the whole document.
      console.warn("graphics card frame:", err);
      this.sent = "";
      fallback();
    } finally {
      this.busy = false;
      const next = this.pending;
      this.pending = null;
      next?.();
    }
  }
}
