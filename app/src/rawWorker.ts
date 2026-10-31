/** The raw reader's worker: reads a camera's raw file to its mosaic,
 * packed for the engine (`core/rawin`). A worker of its own because the
 * reader relies on catching its own panics, which WebAssembly cannot —
 * one would stop whatever module it happened in, and here that is only
 * this worker, which the page then replaces (`src/raw.ts`). It also keeps
 * a second of decoding off the page's thread.
 *
 * The module is built for WASI (`wasm32-wasip1`), not with wasm-bindgen:
 * the CR3 decoder asks the clock what time it is, which the browser
 * target's standard library cannot answer and WASI's asks the host. The
 * host is this file, and the seven things WASI asks of it are below.
 * What the reader writes to stderr — a panic's message, before the
 * module stops — is kept, so a failure can say what it was. */

type ReadRequest = { id: number; bytes: Uint8Array };

type RawExports = {
  memory: WebAssembly.Memory;
  _initialize?: () => void;
  ck_alloc(len: number): number;
  ck_free(ptr: number, len: number): void;
  read_raw(ptr: number, len: number): number;
  result_ptr(): number;
  result_len(): number;
  result_free(): void;
};

let stderr = "";
let exportsReady: Promise<RawExports> | null = null;

function instantiate(): Promise<RawExports> {
  let memory: WebAssembly.Memory | null = null;
  // A fresh view each time: the memory's buffer is replaced as it grows.
  const view = () => new DataView(memory!.buffer);
  const wasi = {
    random_get(ptr: number, len: number) {
      crypto.getRandomValues(new Uint8Array(memory!.buffer, ptr, len));
      return 0;
    },
    environ_sizes_get(count: number, size: number) {
      view().setUint32(count, 0, true);
      view().setUint32(size, 0, true);
      return 0;
    },
    environ_get() {
      return 0;
    },
    clock_time_get(_id: number, _precision: bigint, out: number) {
      view().setBigUint64(out, BigInt(Math.round(performance.now() * 1e6)), true);
      return 0;
    },
    fd_write(fd: number, iovs: number, count: number, written: number) {
      let n = 0;
      for (let i = 0; i < count; i++) {
        const ptr = view().getUint32(iovs + i * 8, true);
        const len = view().getUint32(iovs + i * 8 + 4, true);
        if (fd === 2) stderr += new TextDecoder().decode(new Uint8Array(memory!.buffer, ptr, len));
        n += len;
      }
      view().setUint32(written, n, true);
      return 0;
    },
    proc_exit(code: number) {
      throw new Error(`the raw reader stopped (${code})`);
    },
    sched_yield() {
      return 0;
    },
  };
  return WebAssembly.instantiateStreaming(
    fetch(new URL("./wasm-raw/chitrakar_raw_in.wasm", import.meta.url)),
    { wasi_snapshot_preview1: wasi },
  ).then(({ instance }) => {
    const x = instance.exports as unknown as RawExports;
    memory = x.memory;
    x._initialize?.();
    return x;
  });
}

self.onmessage = async (e: MessageEvent<ReadRequest>) => {
  const { id, bytes } = e.data;
  const post = (msg: object, transfer: Transferable[] = []) =>
    (self as unknown as Worker).postMessage(msg, transfer);
  stderr = "";
  try {
    exportsReady ??= instantiate();
    const x = await exportsReady;
    const p = x.ck_alloc(bytes.length);
    new Uint8Array(x.memory.buffer, p, bytes.length).set(bytes);
    const ok = x.read_raw(p, bytes.length);
    x.ck_free(p, bytes.length);
    const out = new Uint8Array(x.memory.buffer, x.result_ptr(), x.result_len()).slice();
    x.result_free();
    if (ok) post({ id, packed: out }, [out.buffer]);
    else post({ id, error: new TextDecoder().decode(out) });
  } catch (err) {
    // The module stopped: what it said on the way out, if it said
    // anything, is the better account.
    const said = stderr.match(/panicked at [^\n]*\n([^\n]+)/)?.[1] ?? (err instanceof Error ? err.message : String(err));
    post({ id, error: `camera raw: ${said}` });
  }
};

export {};
