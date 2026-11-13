/** The few things the raw reader's WebAssembly module asks of its host
 * through WASI, for the reader's worker and its threads alike
 * (`rawWorker.ts`, `rawThread.ts`).
 *
 * The memory may be shared between workers (the threaded build), and a
 * browser will not decode text out of, or put random bytes into, a view
 * of shared memory, so both go through a copy. */

export type Wasi = Record<string, (...args: never[]) => number>;

/** The host's functions over `memory()` (read fresh each time: the
 * buffer is replaced as the memory grows). What the module writes to
 * stderr goes to `onStderr`; `env` is its environment, as `NAME=value`. */
export function wasiImports(
  memory: () => WebAssembly.Memory,
  onStderr: (text: string) => void,
  env: string[] = [],
): Wasi {
  const view = () => new DataView(memory().buffer);
  const bytes = (ptr: number, len: number) => new Uint8Array(memory().buffer, ptr, len);
  const encoded = env.map((e) => new TextEncoder().encode(`${e}\0`));
  return {
    random_get(ptr: number, len: number) {
      const got = new Uint8Array(len);
      crypto.getRandomValues(got);
      bytes(ptr, len).set(got);
      return 0;
    },
    environ_sizes_get(count: number, size: number) {
      view().setUint32(count, encoded.length, true);
      view().setUint32(
        size,
        encoded.reduce((n, e) => n + e.length, 0),
        true,
      );
      return 0;
    },
    environ_get(pointers: number, buffer: number) {
      let at = buffer;
      encoded.forEach((e, i) => {
        view().setUint32(pointers + i * 4, at, true);
        bytes(at, e.length).set(e);
        at += e.length;
      });
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
        if (fd === 2) onStderr(new TextDecoder().decode(bytes(ptr, len).slice()));
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
  } as unknown as Wasi;
}

/** The memory a module imports, as it declares it: its least and most
 * pages, and whether it is shared — read from the module's import
 * section, since the JavaScript API does not say. */
export function importedMemory(
  wasm: ArrayBuffer,
): { initial: number; maximum?: number; shared: boolean } | null {
  const b = new Uint8Array(wasm);
  let i = 8;
  const leb = () => {
    let v = 0;
    let shift = 0;
    for (;;) {
      const x = b[i++];
      v += (x & 0x7f) * 2 ** shift;
      shift += 7;
      if (x < 0x80) return v;
    }
  };
  const name = () => {
    const n = leb();
    i += n;
  };
  while (i < b.length) {
    const id = b[i++];
    const size = leb();
    const end = i + size;
    if (id === 2) {
      const n = leb();
      for (let k = 0; k < n; k++) {
        name();
        name();
        const kind = b[i++];
        if (kind === 0) leb();
        else if (kind === 1) {
          i++;
          const flags = b[i++];
          leb();
          if (flags & 1) leb();
        } else if (kind === 2) {
          const flags = b[i++];
          const initial = leb();
          const maximum = flags & 1 ? leb() : undefined;
          return { initial, maximum, shared: (flags & 2) !== 0 };
        } else if (kind === 3) i += 2;
        else return null;
      }
      return null;
    }
    i = end;
  }
  return null;
}
