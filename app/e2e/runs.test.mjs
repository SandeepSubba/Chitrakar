// Unit tests for `src/runs.ts` — the algebra of styling part of a text
// block.
//
// This is the one piece of the app that is pure arithmetic over bytes
// and can be asked about without a browser, and it is the piece most
// likely to be quietly wrong: the engine holds a run as a byte range,
// a textarea hands out UTF-16 offsets, and every keystroke in a text
// block runs the whole list through `shiftRuns`. Getting that wrong
// does not throw — it moves somebody's bold onto the wrong letters, or
// puts a range boundary in the middle of a character.
//
// TypeScript, so esbuild (already here, under vite) transpiles the file
// to a temporary module and this imports that. No new dependency, and
// nothing between the test and the source.
//
//   node e2e/runs.test.mjs
import { transform } from "esbuild";
import { readFileSync, writeFileSync, unlinkSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const src = readFileSync(join(here, "..", "src", "runs.ts"), "utf8");
// The only import is `import type`, which the transpile drops.
const js = (await transform(src, { loader: "ts", format: "esm" })).code;
const tmp = join(here, "out", "runs.generated.mjs");
writeFileSync(tmp, js);
const { byteAt, shiftRuns, styleRange, rangeSays } = await import(
  "file://" + tmp
);
unlinkSync(tmp);

let ran = 0;
const fails = [];
const check = (cond, msg) => {
  ran += 1;
  if (!cond) fails.push(msg);
};

const enc = new TextEncoder();
const dec = new TextDecoder();
const bytes = (s) => enc.encode(s);
/** The text a byte range covers, or null when it does not land on
 * character boundaries — which is the failure worth catching. */
const slice = (s, from, to) => {
  const u = bytes(s);
  if (from < 0 || to > u.length || from > to) return null;
  const whole = (i) => i === u.length || (u[i] & 0xc0) !== 0x80;
  if (!whole(from) || !whole(to)) return null;
  return dec.decode(u.slice(from, to));
};

// ---------------------------------------------------------------- byteAt
check(byteAt("hello", 0) === 0, "byteAt at the start is zero");
check(byteAt("hello", 5) === 5, "ASCII counts one byte a letter");
check(byteAt("héllo", 2) === 3, "an accented letter is two bytes");
check(byteAt("a😀b", 3) === 5, "an emoji is four bytes and two code units");
check(byteAt("😀", 2) === 4, "and the whole of it is four");

// ------------------------------------------------------------- shiftRuns
{
  const run = (start, end) => ({ start, end, bold: true });
  // Typing in front of a run moves it along.
  let out = shiftRuns("world", "hello world", [run(0, 5)]);
  check(
    out.length === 1 && out[0].start === 6 && out[0].end === 11,
    `text in front carries the run along (${JSON.stringify(out)})`,
  );
  // Typing behind it leaves it alone.
  out = shiftRuns("world", "world!", [run(0, 5)]);
  check(
    out.length === 1 && out[0].start === 0 && out[0].end === 5,
    `text behind leaves it where it was (${JSON.stringify(out)})`,
  );
  // Deleting the whole of a run drops it rather than leaving an empty
  // range behind.
  out = shiftRuns("a bold word", "a  word", [run(2, 6)]);
  check(out.length === 0, `a run whose text went is dropped (${JSON.stringify(out)})`);
  // An edit inside a run shortens it rather than moving it.
  out = shiftRuns("abcdef", "abf", [run(0, 6)]);
  check(
    out.length === 1 && out[0].start === 0 && out[0].end === 3,
    `an edit inside shortens it (${JSON.stringify(out)})`,
  );
  // Nothing typed, nothing moved.
  const same = [run(1, 3)];
  check(shiftRuns("abc", "abc", same) === same, "no edit hands the runs straight back");
}

// A run's own text survives an edit that does not touch it.
//
// Written as a property over random strings and random edits rather
// than as a handful of cases: the arithmetic is about bytes, the text
// is about characters, and the two only part company on input somebody
// has to have thought to write down. Emoji and accents are in the
// alphabet on purpose.
{
  let seed = 0x2f6e2b1;
  const rnd = () => {
    seed = (seed * 1103515245 + 12345) & 0x7fffffff;
    return seed / 0x7fffffff;
  };
  const pick = (a) => a[Math.floor(rnd() * a.length)];
  const ALPHABET = [..."abcde ", "é", "ü", "😀", "→", "字"];
  const words = (n) =>
    Array.from({ length: n }, () => pick(ALPHABET)).join("");

  for (let round = 0; round < 4000; round += 1) {
    const before = words(1 + Math.floor(rnd() * 14));
    const u = bytes(before);
    // A run on a character boundary, as the app only ever makes.
    const cuts = [];
    for (let i = 0; i <= u.length; i += 1) {
      if (i === u.length || (u[i] & 0xc0) !== 0x80) cuts.push(i);
    }
    const [i, j] = [
      Math.floor(rnd() * cuts.length),
      Math.floor(rnd() * cuts.length),
    ].sort((x, y) => x - y);
    if (cuts[i] === cuts[j]) continue;
    const r = { start: cuts[i], end: cuts[j], bold: true };
    const covered = slice(before, r.start, r.end);

    // An edit: replace a stretch of characters with other ones, which
    // is what a keystroke, a paste and a deletion all are.
    const [p, q] = [
      Math.floor(rnd() * cuts.length),
      Math.floor(rnd() * cuts.length),
    ].sort((x, y) => x - y);
    const text = [...before];
    const at = (byteCut) => {
      // Characters up to a byte cut.
      let n = 0;
      let b = 0;
      for (const ch of text) {
        if (b >= byteCut) break;
        b += bytes(ch).length;
        n += 1;
      }
      return n;
    };
    const inserted = words(Math.floor(rnd() * 4));
    const after =
      text.slice(0, at(cuts[p])).join("") +
      inserted +
      text.slice(at(cuts[q])).join("");

    const [out] = shiftRuns(before, after, [r]);
    if (out === undefined) continue;
    const got = slice(after, out.start, out.end);
    check(
      got !== null,
      `a run must land on character boundaries: ${JSON.stringify({
        before,
        after,
        r,
        out,
      })}`,
    );
    // Where the edit is wholly after the run, or wholly before it, the
    // run still covers exactly the text it covered.
    //
    // "The edit" means the one the function can see, not the one this
    // test meant. Typing an "e" in front of an existing "e" is the same
    // pair of strings as typing it behind, and nothing can tell them
    // apart — so the property is stated against the longest common
    // prefix and suffix, which is how the function reads the edit.
    const [ub, ua] = [bytes(before), bytes(after)];
    let editFrom = 0;
    while (
      editFrom < ub.length &&
      editFrom < ua.length &&
      ub[editFrom] === ua[editFrom]
    )
      editFrom += 1;
    let back = 0;
    while (
      back < ub.length - editFrom &&
      back < ua.length - editFrom &&
      ub[ub.length - 1 - back] === ua[ua.length - 1 - back]
    )
      back += 1;
    const editTo = ub.length - back;
    const clear = editFrom >= r.end || editTo <= r.start;
    if (clear && got !== null) {
      check(
        got === covered,
        `an edit outside a run leaves its text alone: ${JSON.stringify({
          before,
          after,
          r,
          out,
          covered,
          got,
        })}`,
      );
    }
    check(out.end > out.start, "a run that survives is not empty");
  }
}

// --------------------------------------------------------- styleRange
{
  // Styling a stretch of an unstyled block puts one run there.
  const out = styleRange("hello world", [], 6, 11, { bold: true });
  check(
    out.length === 1 && out[0].start === 6 && out[0].end === 11 && out[0].bold === true,
    `styling a stretch makes one run (${JSON.stringify(out)})`,
  );
  // A run saying what the block already says is not a run: left behind
  // it would swallow the next Bold pressed on the block itself.
  const same = styleRange("hello", [], 0, 5, { bold: true }, { bold: true });
  check(
    same.length === 0,
    `a run that agrees with the block is dropped (${JSON.stringify(same)})`,
  );
  // Two touching runs that say the same thing become one.
  const joined = styleRange("hello", [], 0, 3, { bold: true });
  const both = styleRange("hello", joined, 3, 5, { bold: true });
  check(
    both.length === 1 && both[0].start === 0 && both[0].end === 5,
    `touching runs that agree are one run (${JSON.stringify(both)})`,
  );
}

// ---------------------------------------------------------- rangeSays
{
  const runs = styleRange("hello world", [], 0, 5, { bold: true });
  check(rangeSays("hello world", runs, 0, 5, "bold", false) === true, "all of it bold");
  check(
    rangeSays("hello world", runs, 6, 11, "bold", false) === false,
    "and none of the rest",
  );
  check(
    rangeSays("hello world", runs, 0, 11, "bold", false) === undefined,
    "a range of two minds says nothing",
  );
}

for (const f of fails) console.error("FAIL: " + f);
console.log(`${ran - fails.length} of ${ran} checks passed`);
if (fails.length > 0) process.exit(1);
console.log("ALL RUNS TESTS PASSED");
