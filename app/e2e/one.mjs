// Run one block of the smoke suite on its own.
//
// The suite is one long script: every block builds on the document the
// last one left, so there is nothing to run in isolation — and a full
// run takes a quarter of an hour, which is a long time to wait to find
// out that a threshold was a point too tight. Almost every block does
// begin by making its own document, though, so most of them will run
// against the harness alone.
//
// This puts that together: the harness (everything before the first
// block), the helper functions declared between blocks, and the one
// block asked for.
//
//   node e2e/one.mjs 9af                 # by its number
//   node e2e/one.mjs "Colour balance"    # or by words from its heading
//
// A block that needs what an earlier one left behind will fail here and
// pass in the suite; that is this tool's limit rather than a fault in
// the block. The suite itself is the gate — this is for the loop before
// it.
import { spawn } from "node:child_process";
import { readFileSync, writeFileSync, unlinkSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const want = process.argv[2];
if (!want) {
  console.error("usage: node e2e/one.mjs <block number, or words from its heading>");
  process.exit(2);
}

const src = readFileSync(join(here, "smoke.mjs"), "utf8");

/** Where each block starts: a comment at the left margin opening with
 * the block's number. */
const heads = [...src.matchAll(/^\/\/ (\d+[a-z]*)\. .*$/gm)];
if (heads.length === 0) throw new Error("no blocks found in smoke.mjs");

const hit =
  heads.find((m) => m[1] === want) ??
  heads.find((m) => m[0].toLowerCase().includes(want.toLowerCase()));
if (!hit) {
  console.error(`no block matches "${want}". Blocks:`);
  for (const m of heads) console.error("  " + m[0].slice(3, 78));
  process.exit(2);
}

const from = hit.index;
const next = heads.find((m) => m.index > from);
const block = src.slice(from, next ? next.index : src.length);

// The harness: everything before the first block.
const harness = src.slice(0, heads[0].index);

// Plus the helper functions declared between blocks — setColor, pickTool
// and the rest were written where they were first wanted rather than at
// the top. Functions only: a `const` holding a measurement taken from
// the document describes a document this run will not have.
const helpers = [];
for (const m of src.matchAll(/^const \w+ = (?:async )?\(.*?=> \{$/gm)) {
  if (m.index < heads[0].index || m.index >= from) continue;
  const end = src.indexOf("\n};\n", m.index);
  if (end > 0) helpers.push(src.slice(m.index, end + 4));
}

const out = join(here, "_one.generated.mjs");
writeFileSync(
  out,
  harness +
    helpers.join("\n") +
    "\n" +
    block +
    '\nconsole.log("\\nBLOCK PASSED", errors);\n' +
    "await browser.close();\nserver.close();\nprocess.exit(0);\n",
);

const run = spawn(process.execPath, [out], {
  stdio: "inherit",
  cwd: join(here, ".."),
});
run.on("exit", (code) => {
  try {
    unlinkSync(out);
  } catch {
    /* leaving it behind is harmless */
  }
  process.exit(code ?? 1);
});
