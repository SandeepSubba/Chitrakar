// Unit tests for `src/spacing.ts` — the equal-spacing guides the Move
// tool snaps to. Pure arithmetic over boxes, asked without a browser.
//
//   node e2e/spacing.test.mjs
import { build } from "esbuild";
import { unlinkSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const tmp = join(here, "out", "spacing.generated.mjs");
await build({
  entryPoints: [join(here, "..", "src", "spacing.ts")],
  bundle: true,
  format: "esm",
  outfile: tmp,
  logLevel: "silent",
});
const { equalSpacing } = await import("file://" + tmp);
unlinkSync(tmp);

let ran = 0;
const fails = [];
const check = (cond, msg) => {
  ran += 1;
  if (!cond) fails.push(msg);
};
const near = (a, b) => Math.abs(a - b) < 1e-9;
const lengths = (s) => (s ? s.marks.map((k) => k.length) : []);

// Two boxes in a row, 20 apart; a third carried past the second, 23
// after it: it snaps back 3 to leave 20, and both gaps are marked.
{
  const a = [0, 0, 10, 10];
  const b = [30, 0, 40, 10];
  const s = equalSpacing([63, 2, 73, 12], [a, b], "x", 5);
  check(s && near(s.delta, -3), `past the second, the same gap: ${JSON.stringify(s)}`);
  check(String(lengths(s)) === "20,20", `both gaps marked: ${lengths(s)}`);
  check(s && near(s.marks[1].from[1], 6), `drawn through what they share, 2..10: ${s && s.marks[1].from}`);
  check(equalSpacing([63, 2, 73, 12], [a, b], "x", 2) === null, "too far to catch");
}
// The mirror: carried before the first.
{
  const s = equalSpacing([-28, 0, -18, 10], [[0, 0, 10, 10], [30, 0, 40, 10]], "x", 5);
  check(s && near(s.delta, -2), `before the first, the same gap: ${JSON.stringify(s)}`);
}
// Between two: the middle. 80 of room less 10 of box is 35 a side, so
// a box at 42 goes 3 further, to 45.
{
  const s = equalSpacing([42, 0, 52, 10], [[0, 0, 10, 10], [90, 0, 100, 10]], "x", 5);
  check(s && near(s.delta, 3) && String(lengths(s)) === "35,35", `between, the middle: ${JSON.stringify(s)}`);
}
// Down a column, the same.
{
  const s = equalSpacing([0, 61, 10, 71], [[0, 0, 10, 10], [2, 30, 12, 40]], "y", 5);
  check(s && near(s.delta, -1) && String(lengths(s)) === "20,20", `down a column: ${JSON.stringify(s)}`);
}
// Only what is beside it counts: a box in another row is not a
// neighbour, and one box alone has no gap to repeat.
{
  const other = equalSpacing([63, 50, 73, 60], [[0, 0, 10, 10], [30, 0, 40, 10]], "x", 5);
  check(other === null, `another row is not beside it: ${JSON.stringify(other)}`);
  const lone = equalSpacing([33, 0, 43, 10], [[0, 0, 10, 10]], "x", 5);
  check(lone === null, `one neighbour has no gap to repeat: ${JSON.stringify(lone)}`);
}
// Already in place, it stays, and still says so.
{
  const s = equalSpacing([60, 0, 70, 10], [[0, 0, 10, 10], [30, 0, 40, 10]], "x", 5);
  check(s && near(s.delta, 0) && String(lengths(s)) === "20,20", `in place: ${JSON.stringify(s)}`);
}

if (fails.length) {
  console.error(`${fails.length} of ${ran} failed:\n  ` + fails.join("\n  "));
  process.exit(1);
}
console.log(`spacing: ${ran} checks passed`);
