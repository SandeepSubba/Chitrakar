/** The raw reader's worker: reads a camera's raw file to its mosaic,
 * packed for the engine (`core/rawin`). A worker of its own because the
 * reader relies on catching its own panics, which WebAssembly cannot —
 * one would stop whatever module it happened in, and here that is only
 * this worker, which the page then replaces (`src/raw.ts`). It also keeps
 * the decoding off the page's thread.
 *
 * The module is built for WASI (`wasm32-wasip1`), not with wasm-bindgen:
 * the CR3 decoder asks the clock what time it is, which the browser
 * target's standard library cannot answer and WASI's asks the host. The
 * host is this file and `wasiHost.ts`. What the reader writes to stderr
 * — a panic's message, before the module stops — is kept, so a failure
 * can say what it was.
 *
 * Where the page is cross-origin isolated it is given threads first
 * (`kind: "threads"`, one channel to each of the page's `rawThread.ts`
 * workers) and loads the threaded build (`wasm32-wasip1-threads`), whose
 * memory is shared with them: rawler decodes Fuji's compressed RAF,
 * Canon's CR2 and CR3 and others a strip to a thread, through rayon, and
 * each thread rayon starts (`wasi.thread-spawn`) is started on one of
 * them. rayon is told how many there are (RAYON_NUM_THREADS), so it
 * starts no more.
 *
 * A thread is started from inside a read, while this worker is busy and
 * cannot hand a message on, so it is not started by a message: each
 * thread's worker waits on its own slot of a small shared array, and
 * starting it is writing the slot and waking it (`Atomics.notify`). */

import { importedMemory, wasiImports } from "./wasiHost";

type ReadRequest = { id: number; bytes: Uint8Array } | { kind: "threads"; ports: MessagePort[] };

type RawExports = {
  memory?: WebAssembly.Memory;
  _initialize?: () => void;
  /** The threaded build's start-up, which a library module is not given
   * by its C runtime: the main thread's own record (which every lock
   * reads its owner from), then the constructors. */
  __wasi_init_tp?: () => void;
  __wasm_call_ctors?: () => void;
  ck_alloc(len: number): number;
  ck_free(ptr: number, len: number): void;
  read_raw(ptr: number, len: number): number;
  result_ptr(): number;
  result_len(): number;
  result_free(): void;
};

let stderr = "";
let ports: MessagePort[] = [];
let exportsReady: Promise<{ x: RawExports; memory: WebAssembly.Memory }> | null = null;

async function instantiate(): Promise<{ x: RawExports; memory: WebAssembly.Memory }> {
  if (ports.length === 0) {
    let memory: WebAssembly.Memory | null = null;
    const { instance } = await WebAssembly.instantiateStreaming(
      fetch(new URL("./wasm-raw/chitrakar_raw_in.wasm", import.meta.url)),
      { wasi_snapshot_preview1: wasiImports(() => memory!, (t) => (stderr += t)) },
    );
    const x = instance.exports as unknown as RawExports;
    memory = x.memory!;
    x._initialize?.();
    return { x, memory };
  }
  const bytes = await (
    await fetch(new URL("./wasm-raw/chitrakar_raw_in_threads.wasm", import.meta.url))
  ).arrayBuffer();
  const declared = importedMemory(bytes);
  if (!declared?.shared) throw new Error("the threaded raw reader does not share its memory");
  const module = await WebAssembly.compile(bytes);
  const memory = new WebAssembly.Memory({
    initial: declared.initial,
    maximum: declared.maximum,
    shared: true,
  });
  const env = [`RAYON_NUM_THREADS=${ports.length}`];
  // Two numbers a thread: whether it has been started, and with what.
  const slots = new Int32Array(new SharedArrayBuffer(ports.length * 8));
  let next = 1;
  const instance = await WebAssembly.instantiate(module, {
    env: { memory },
    wasi_snapshot_preview1: wasiImports(() => memory, (t) => (stderr += t), env),
    wasi: {
      // Called from inside a read, which cannot wait: the thread's worker
      // is already there and is only told to start.
      "thread-spawn": (arg: number) => {
        const tid = next++;
        const i = tid - 1;
        if (i >= ports.length) return -1;
        Atomics.store(slots, i * 2 + 1, arg);
        Atomics.store(slots, i * 2, 1);
        Atomics.notify(slots, i * 2);
        return tid;
      },
    },
  });
  const started = instance.exports as unknown as RawExports;
  started.__wasi_init_tp?.();
  started.__wasm_call_ctors?.();
  // Every thread's worker has the module and the memory before a read
  // can ask for one.
  await Promise.all(
    ports.map(
      (port) =>
        new Promise<void>((done) => {
          port.onmessage = (m) => {
            if (m.data?.kind === "ready") done();
          };
          port.postMessage({ kind: "init", module, memory, env, slots, index: ports.indexOf(port) });
        }),
    ),
  );
  return { x: started, memory };
}

self.onmessage = async (e: MessageEvent<ReadRequest>) => {
  const post = (msg: object, transfer: Transferable[] = []) =>
    (self as unknown as Worker).postMessage(msg, transfer);
  if ("kind" in e.data) {
    ports = e.data.ports;
    return;
  }
  const { id, bytes } = e.data;
  stderr = "";
  try {
    exportsReady ??= instantiate();
    const { x, memory } = await exportsReady;
    const p = x.ck_alloc(bytes.length);
    new Uint8Array(memory.buffer, p, bytes.length).set(bytes);
    const ok = x.read_raw(p, bytes.length);
    x.ck_free(p, bytes.length);
    const out = new Uint8Array(memory.buffer, x.result_ptr(), x.result_len()).slice();
    x.result_free();
    if (ok) post({ id, packed: out, threads: ports.length }, [out.buffer]);
    else post({ id, error: new TextDecoder().decode(out) });
  } catch (err) {
    // The module stopped: what it said on the way out, if it said
    // anything, is the better account.
    const said =
      stderr.match(/panicked at [^\n]*\n([^\n]+)/)?.[1] ?? (err instanceof Error ? err.message : String(err));
    post({ id, error: `camera raw: ${said}` });
  }
};

export {};
