// The engine's threaded build, which the develop worker runs
// (`src/developWorker.ts`): its memory shared with a pool of workers, so
// a raw file is developed on every core.
//
// Shared memory means building the standard library again with atomics,
// which `-Z build-std` does. That is a nightly flag; RUSTC_BOOTSTRAP lets
// the toolchain pinned in rust-toolchain.toml take it (with its rust-src
// component), so the threaded build is the same compiler as the rest
// rather than a second toolchain to keep in step. Its own target folder
// keeps the plain build's cache: the two are built with different flags.
//
// RUSTFLAGS replaces .cargo/config.toml's, so SIMD is asked for again
// here, with the atomics. The memory may grow to 4 GB (the default for
// shared memory is 1 GB, which a big photograph and its development can
// pass).
//
// It goes into public/, served as it is rather than bundled: the pool
// starts its workers from its own file (`new Worker(new URL(...,
// import.meta.url))`), which a bundler turns into the URL of whichever
// script imported it. Served as it is, that resolves as written; the one
// thing a browser cannot follow is the pool's import of the folder its
// module is in, which is pointed at the module itself below.
import { spawnSync } from "node:child_process";
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

const env = {
  ...process.env,
  RUSTC_BOOTSTRAP: "1",
  // Absolute: cargo runs in the engine's folder, not this one.
  CARGO_TARGET_DIR: resolve("../target/wasm-threads"),
  RUSTFLAGS: [
    "-C target-feature=+atomics,+bulk-memory,+mutable-globals,+simd128",
    // The linker is told to share the memory and take it from outside
    // (the pool's workers are given the page's), and to export what
    // wasm-bindgen needs to give each thread its own statics; the
    // compiler no longer passes these itself.
    "-C link-arg=--shared-memory",
    "-C link-arg=--import-memory",
    "-C link-arg=--max-memory=4294967296",
    "-C link-arg=--export=__wasm_init_tls",
    "-C link-arg=--export=__tls_size",
    "-C link-arg=--export=__tls_align",
    "-C link-arg=--export=__tls_base",
  ].join(" "),
};
const args = [
  "build",
  "../core/engine",
  "--target",
  "web",
  "--release",
  "--out-dir",
  "../../app/public/wasm-threads",
  "--",
  "--features",
  "threads",
  "-Z",
  "build-std=panic_abort,std",
];
const run = spawnSync("wasm-pack", args, { env, stdio: "inherit", shell: process.platform === "win32" });
if (run.status !== 0) process.exit(run.status ?? 1);

const snippets = "public/wasm-threads/snippets";
let patched = 0;
for (const dir of readdirSync(snippets)) {
  if (!dir.startsWith("wasm-bindgen-rayon-")) continue;
  const file = join(snippets, dir, "src", "workerHelpers.js");
  const before = readFileSync(file, "utf8");
  const after = before.replace("import('../../..')", "import('../../../chitrakar_engine.js')");
  if (after === before && !before.includes("../../../chitrakar_engine.js")) {
    console.error(`build-threads: the pool's import of its module was not found in ${file}`);
    process.exit(1);
  }
  writeFileSync(file, after);
  patched += 1;
}
if (patched !== 1) {
  console.error(`build-threads: expected one wasm-bindgen-rayon snippet, found ${patched}`);
  process.exit(1);
}
