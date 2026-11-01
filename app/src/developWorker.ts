/** The develop worker: develops a camera's raw file in full on as many
 * threads as the machine has, off the page's own thread.
 *
 * It loads the engine's threaded build (`public/wasm-threads`, built by
 * `npm run build:wasm-threads`), whose memory is shared with a pool of
 * workers of its own (`initThreadPool`, wasm-bindgen-rayon). That build
 * cannot run on the page: rayon's threads wait for one another, and a
 * page's thread is not allowed to wait. So the page keeps the plain
 * engine for everything else and sends this worker the packed mosaic and
 * the settings; it opens the mosaic in an engine of its own, develops it
 * and sends back the pixels, which the page's engine places
 * (`place_raw_pixels`). Only where the page is cross-origin isolated —
 * the one condition under which memory can be shared (`src/developer.ts`).
 *
 * The lens profiles a photograph without corrections of its own is
 * looked up in come with each request when they have changed, so a
 * development here carries the same lens as the page's preview. */

type Engine = typeof import("../public/wasm-threads/chitrakar_engine");
type WasmSession = InstanceType<Engine["WasmSession"]>;

/** The threaded build, served as it is beside the page (see
 * `tools/build-threads.mjs` for why it is not bundled): from this
 * worker's own folder (src/ while developing, assets/ when built), one
 * up. */
const engine = (): Promise<Engine> =>
  import(/* @vite-ignore */ new URL("../wasm-threads/chitrakar_engine.js", self.location.href).href);

export type DevelopRequest =
  | { id: number; kind: "start"; threads: number }
  | {
      id: number;
      kind: "develop";
      packed: Uint8Array;
      name: string;
      settings: string;
      /** The lens profiles' texts, when they are not the set sent last. */
      profiles?: string[];
    };

export type DevelopReply =
  | { id: number; threads: number }
  | { id: number; rgba: Uint8Array; width: number; height: number }
  | { id: number; error: string };

let ready: Promise<number> | null = null;
let session: WasmSession | null = null;
let profiles: string[] = [];

function start(threads: number): Promise<number> {
  ready ??= (async () => {
    const m = await engine();
    await m.default();
    await m.initThreadPool(Math.max(1, threads));
    session = new m.WasmSession(1, 1, false);
    return threads;
  })();
  return ready;
}

self.onmessage = async (e: MessageEvent<DevelopRequest>) => {
  const req = e.data;
  const post = (reply: DevelopReply, transfer: Transferable[] = []) =>
    (self as unknown as Worker).postMessage(reply, transfer);
  try {
    if (req.kind === "start") {
      post({ id: req.id, threads: await start(req.threads) });
      return;
    }
    await start(1);
    const s = session!;
    if (req.profiles) {
      profiles = req.profiles;
      s.clear_lens_profiles();
      for (const text of profiles) {
        try {
          s.add_lens_profiles(text);
        } catch {
          // The page refused it too; it is not in its set either.
        }
      }
    }
    const info = JSON.parse(s.raw_open(req.packed, new Uint8Array(0), req.name)) as {
      width: number;
      height: number;
    };
    try {
      const rgba = s.raw_develop(req.settings);
      post({ id: req.id, rgba, width: info.width, height: info.height }, [rgba.buffer]);
    } finally {
      s.raw_close();
    }
  } catch (err) {
    post({ id: req.id, error: err instanceof Error ? err.message : String(err) });
  }
};
