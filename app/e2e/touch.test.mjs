// Unit tests for `src/touch.ts` — two- and three-finger taps, and the
// modifier keys the screen holds for a hand with no keyboard. Pure
// bookkeeping over pointer events, so it is asked here without a
// browser, transpiled the way the runs test is.
//
//   node e2e/touch.test.mjs
import { transform } from "esbuild";
import { readFileSync, writeFileSync, unlinkSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const src = readFileSync(join(here, "..", "src", "touch.ts"), "utf8");
const js = (await transform(src, { loader: "ts", format: "esm" })).code;
const tmp = join(here, "out", "touch.generated.mjs");
writeFileSync(tmp, js);
const { FingerTaps, wear, afterPress, NONE_HELD } = await import("file://" + tmp);
unlinkSync(tmp);

let ran = 0;
const fails = [];
const check = (cond, msg) => {
  ran += 1;
  if (!cond) fails.push(msg);
};

// Two fingers down and up together, quickly, still: a two-finger tap,
// said once, when the last one lifts.
{
  const t = new FingerTaps();
  t.down(1, 100, 100, 0);
  t.down(2, 200, 100, 20);
  check(t.up(1, 120) === 0, "nothing is said while a finger is still down");
  check(t.up(2, 140) === 2, "two fingers tapped");
}
// Three, landing one after another and lifting in any order.
{
  const t = new FingerTaps();
  t.down(1, 100, 100, 0);
  t.down(2, 150, 100, 30);
  t.down(3, 200, 100, 60);
  check(t.up(2, 150) === 0 && t.up(3, 160) === 0, "three: quiet until the last");
  check(t.up(1, 200) === 3, "three fingers tapped");
}
// The count is the most that were ever down, not how many lifted last.
{
  const t = new FingerTaps();
  t.down(1, 0, 0, 0);
  t.down(2, 50, 0, 10);
  t.up(1, 40);
  check(t.up(2, 80) === 2, "staggered lifting is still two");
}
// A finger that travels makes it a pinch or a drag, not a tap.
{
  const t = new FingerTaps();
  t.down(1, 100, 100, 0);
  t.down(2, 200, 100, 0);
  t.move(1, 80, 100);
  t.up(1, 100);
  check(t.up(2, 120) === 0, "a finger that travelled twenty spoils it");
}
// A little wander is still a tap: fingers are not still.
{
  const t = new FingerTaps();
  t.down(1, 100, 100, 0);
  t.down(2, 200, 100, 0);
  t.move(1, 106, 104);
  t.move(2, 195, 98);
  t.up(1, 100);
  check(t.up(2, 120) === 2, "a few pixels of wander is still a tap");
}
// Slow is a hold, not a tap.
{
  const t = new FingerTaps();
  t.down(1, 100, 100, 0);
  t.down(2, 200, 100, 0);
  t.up(1, 500);
  check(t.up(2, 520) === 0, "fingers held half a second are not a tap");
}
// The window is from the first landing to the last lifting.
{
  const t = new FingerTaps();
  t.down(1, 100, 100, 0);
  t.down(2, 200, 100, 300);
  t.up(1, 320);
  check(t.up(2, 400) === 0, "four hundred from first down to last up is too slow");
}
// One finger tapped is said as one, and the editor leaves it to the tool.
{
  const t = new FingerTaps();
  t.down(7, 10, 10, 0);
  check(t.up(7, 50) === 1, "one finger is a one");
}
// A finger the system took back spoils it, and the next gesture starts
// clean.
{
  const t = new FingerTaps();
  t.down(1, 100, 100, 0);
  t.down(2, 200, 100, 0);
  t.spoil(1);
  check(t.up(2, 50) === 0, "a cancelled finger is no tap");
  t.down(3, 0, 0, 1000);
  t.down(4, 40, 0, 1010);
  t.up(3, 1050);
  check(t.up(4, 1060) === 2, "and the next two-finger tap counts again");
}
// A finger lifted that was never seen down says nothing.
{
  const t = new FingerTaps();
  check(t.up(9, 10) === 0, "an unknown finger is ignored");
}

// Wearing the held keys: a plain event says them afterwards.
{
  const e = new Event("pointerdown");
  wear(e, { shift: true, alt: false, mod: true }, false);
  check(e.shiftKey === true, "shift held reads as shift");
  check(e.altKey !== true, "alt not held stays unsaid");
  check(e.ctrlKey === true && e.metaKey !== true, "mod is control off a Mac");
  const m = new Event("pointerdown");
  wear(m, { shift: false, alt: true, mod: true }, true);
  check(m.altKey === true && m.metaKey === true && m.ctrlKey !== true, "and command on one");
  const n = new Event("click");
  wear(n, NONE_HELD, false);
  check(!n.shiftKey && !n.altKey && !n.ctrlKey && !n.metaKey, "nothing held changes nothing");
}

// A modifier key on the screen: tapped it latches, tapped again it lets
// go, and held through a stroke it lets go with the finger.
check(afterPress(false, 80) === true, "a tap latches it");
check(afterPress(true, 80) === false, "a second tap lets it go");
check(afterPress(false, 900) === false, "held and let go, it goes");
check(afterPress(true, 900) === false, "held while latched, it goes too");

if (fails.length) {
  console.error(`touch: ${fails.length} of ${ran} failed`);
  for (const f of fails) console.error("  " + f);
  process.exit(1);
}
console.log(`touch: ${ran} checks passed`);
