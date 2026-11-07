// Unit tests for `src/measure.ts` — the distances the Move tool shows
// between two layers with alt held. Pure arithmetic over boxes, so it
// is asked here without a browser, transpiled the way the runs test is.
//
//   node e2e/measure.test.mjs
import { transform } from "esbuild";
import { readFileSync, writeFileSync, unlinkSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const src = readFileSync(join(here, "..", "src", "measure.ts"), "utf8");
const js = (await transform(src, { loader: "ts", format: "esm" })).code;
const tmp = join(here, "out", "measure.generated.mjs");
writeFileSync(tmp, js);
const { gapsBetween, boxOf, union } = await import("file://" + tmp);
unlinkSync(tmp);

let ran = 0;
const fails = [];
const check = (cond, msg) => {
  ran += 1;
  if (!cond) fails.push(msg);
};
const near = (a, b) => Math.abs(a - b) < 1e-9;
const says = (gaps) => gaps.map((g) => `${g.from}->${g.to}=${g.length}`).join(" ");

// Side by side, level: one gap, through the middle of what they share.
{
  const g = gapsBetween([0, 0, 10, 10], [25, 4, 40, 20]);
  check(g.length === 1, `side by side is one gap: ${says(g)}`);
  check(near(g[0].length, 15) && near(g[0].from[0], 10) && near(g[0].to[0], 25), `from edge to edge: ${says(g)}`);
  check(near(g[0].from[1], 7) && near(g[0].to[1], 7), `through the shared stretch, 4..10: ${says(g)}`);
}
// The other way round, and above and below.
{
  const left = gapsBetween([25, 4, 40, 20], [0, 0, 10, 10]);
  check(left.length === 1 && near(left[0].length, 15) && near(left[0].from[0], 10), `to the left: ${says(left)}`);
  const below = gapsBetween([0, 0, 10, 10], [2, 30, 8, 50]);
  check(below.length === 1 && near(below[0].length, 20), `below: ${says(below)}`);
  check(near(below[0].from[0], 5) && near(below[0].from[1], 10) && near(below[0].to[1], 30), `upright, through 2..8: ${says(below)}`);
}
// Apart on both axes: a gap each way, each line through the middle of
// the picked one, since they share no stretch.
{
  const g = gapsBetween([0, 0, 10, 10], [30, 40, 50, 60]);
  check(g.length === 2, `apart both ways is two gaps: ${says(g)}`);
  check(near(g[0].length, 20) && near(g[0].from[1], 5), `across, through the picked one's middle: ${says(g)}`);
  check(near(g[1].length, 30) && near(g[1].from[0], 5), `down, through the picked one's middle: ${says(g)}`);
}
// Inside: how far in from each side, four of them.
{
  const g = gapsBetween([10, 20, 30, 50], [0, 0, 100, 60]);
  const lengths = g.map((x) => x.length);
  check(g.length === 4, `inside is four distances: ${says(g)}`);
  check(
    near(lengths[0], 10) && near(lengths[1], 70) && near(lengths[2], 20) && near(lengths[3], 10),
    `left, right, top, bottom: ${lengths}`,
  );
  check(near(g[0].from[1], 35) && near(g[2].from[0], 20), `through the inner one's middle: ${says(g)}`);
  // The same asked the other way round: what is picked holds the other.
  const outward = gapsBetween([0, 0, 100, 60], [10, 20, 30, 50]).map((x) => x.length);
  check(String(outward) === String(lengths), `holding is the same distances: ${outward}`);
}
// Overlapping without holding: nothing on that axis; touching: nothing.
{
  const g = gapsBetween([0, 0, 20, 20], [10, 30, 40, 40]);
  check(g.length === 1 && near(g[0].length, 10), `overlapping across says only the gap down: ${says(g)}`);
  const touching = gapsBetween([0, 0, 10, 10], [10, 0, 20, 10]);
  check(touching.length === 0, `touching says nothing: ${says(touching)}`);
  const flush = gapsBetween([0, 0, 10, 10], [0, 0, 30, 10]);
  check(flush.length === 1 && near(flush[0].length, 20), `flush on three sides says the fourth: ${says(flush)}`);
}
// Boxes from the engine's [x, y, w, h], and round several.
{
  const b = boxOf([3, 4, 10, 20]);
  check(String(b) === "3,4,13,24", `a box from x, y, w, h: ${b}`);
  check(boxOf([]) === null && boxOf([0, 0, -1, 2]) === null, "no box from nothing");
  check(String(union([[0, 5, 10, 10], [4, 0, 20, 8]])) === "0,0,20,10", "the box round two");
  check(union([]) === null, "nothing round none");
}

if (fails.length) {
  console.error(`${fails.length} of ${ran} failed:\n  ` + fails.join("\n  "));
  process.exit(1);
}
console.log(`measure: ${ran} checks passed`);
