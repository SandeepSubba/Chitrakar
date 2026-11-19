/** One thread of the raw reader's threaded build (`rawWorker.ts`): the
 * same module instantiated over the reader's shared memory, which runs
 * whatever thread the reader starts on it — rayon's, decoding a strip of
 * a Fuji's compressed RAF or a Canon's CR2 at a time.
 *
 * Made by the page (`raw.ts`), which hands it one end of a channel whose
 * other end the reader holds, down which it is given the module, the
 * memory and its slot of a shared array. It then waits on that slot: the
 * reader is inside a read when it starts a thread, busy, and starts it
 * by writing the slot and waking it rather than by a message it could
 * not hand on. A thread that fails stops this worker, which the page
 * sees and takes as the read failing. What it said on the way out is
 * sent to the page first. */

import { wasiImports } from "./wasiHost";

type ThreadExports = { wasi_thread_start(tid: number, arg: number): void };

type Init = {
  kind: "init";
  module: WebAssembly.Module;
  memory: WebAssembly.Memory;
  env: string[];
  slots: Int32Array;
  index: number;
};

self.onmessage = (e: MessageEvent<{ port: MessagePort }>) => {
  const port = e.data.port;
  port.onmessage = async (m: MessageEvent<Init>) => {
    const { module, memory, env, slots, index } = m.data;
    const instance = await WebAssembly.instantiate(module, {
      env: { memory },
      wasi_snapshot_preview1: wasiImports(
        () => memory,
        (text) => (self as unknown as Worker).postMessage({ stderr: text }),
        env,
      ),
      // A thread starting threads of its own is not something the reader
      // does; refused, it would say so rather than hang.
      wasi: { "thread-spawn": () => -1 },
    });
    const thread = instance.exports as unknown as ThreadExports;
    port.postMessage({ kind: "ready" });
    // Off this message's turn, so the reply goes; then wait for the
    // reader to start this thread, and run it — rayon's, for good.
    setTimeout(() => {
      const at = index * 2;
      Atomics.wait(slots, at, 0);
      thread.wasi_thread_start(index + 1, Atomics.load(slots, at + 1));
    });
  };
};

export {};
