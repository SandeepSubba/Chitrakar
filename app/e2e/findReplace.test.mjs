// Unit tests for `src/findReplace.ts` — Edit › Find and replace, one
// text block at a time. Transpiled the way the runs test is; the module
// imports `runs.ts`, so both go through esbuild into one bundle.
//
//   node e2e/findReplace.test.mjs
import { build } from "esbuild";
import { unlinkSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const tmp = join(here, "out", "findReplace.generated.mjs");
await build({
  entryPoints: [join(here, "..", "src", "findReplace.ts")],
  bundle: true,
  format: "esm",
  outfile: tmp,
  logLevel: "silent",
});
const { matchesIn, replaceIn } = await import("file://" + tmp);
unlinkSync(tmp);

let ran = 0;
const fails = [];
const check = (cond, msg) => {
  ran += 1;
  if (!cond) fails.push(msg);
};
const enc = new TextEncoder();
const bytes = (s) => enc.encode(s).length;
// The run that covers `word` in `text`, as the engine keeps it: bytes.
const runOver = (text, word, extra = { bold: true }) => {
  const at = text.indexOf(word);
  return { start: bytes(text.slice(0, at)), end: bytes(text.slice(0, at + word.length)), ...extra };
};
const covered = (text, run) => new TextDecoder().decode(enc.encode(text).slice(run.start, run.end));
const loose = { matchCase: false, wholeWord: false };

// Finding: case, whole words, nothing for nothing.
{
  const t = "Cat cat category CAT";
  check(matchesIn(t, "cat", loose).length === 4, "any case, anywhere");
  check(matchesIn(t, "cat", { matchCase: true, wholeWord: false }).length === 2, "this case only");
  check(matchesIn(t, "cat", { matchCase: false, wholeWord: true }).length === 3, "whole words only");
  check(matchesIn(t, "cat", { matchCase: true, wholeWord: true }).length === 1, "both");
  check(matchesIn(t, "", loose).length === 0, "nothing finds nothing");
  check(matchesIn("a.b a+b", ".", loose).length === 1, "a dot is a dot, not any character");
  check(matchesIn("aaaa", "aa", loose).length === 2, "never overlapping");
  check(matchesIn("café cafés", "café", { matchCase: false, wholeWord: true }).length === 1, "a word boundary knows accents");
}

// Replacing: the text, the count, and nothing changed when nothing matched.
{
  const r = replaceIn("one fish two fish", [], "fish", "cat", loose);
  check(r.text === "one cat two cat" && r.count === 2, `replaced: ${r.text}`);
  const none = replaceIn("one fish", [], "bird", "cat", loose);
  check(none.text === "one fish" && none.count === 0, "nothing to replace");
  const longer = replaceIn("a b a", [], "a", "alpha", loose);
  check(longer.text === "alpha b alpha", `growing: ${longer.text}`);
}

// Runs: a styled word after the replacements is still that word; a styled
// word replaced is the replacement, styled; a run over accents and emoji
// keeps whole characters.
{
  const text = "the fish and the bold fish";
  const runs = [runOver(text, "bold")];
  const r = replaceIn(text, runs, "fish", "salmon", loose);
  check(covered(r.text, r.runs[0]) === "bold", `the styled word after is still that word: ${covered(r.text, r.runs[0])}`);
  const t2 = "a fish here";
  const r2 = replaceIn(t2, [runOver(t2, "fish")], "fish", "seahorse", loose);
  check(covered(r2.text, r2.runs[0]) === "seahorse", `the styled word replaced is styled: ${covered(r2.text, r2.runs[0])}`);
  const t3 = "é é 🐟 bold";
  const r3 = replaceIn(t3, [runOver(t3, "bold")], "é", "eee", loose);
  check(covered(r3.text, r3.runs[0]) === "bold", `past accents: ${covered(r3.text, r3.runs[0])}`);
  const r4 = replaceIn(t3, [runOver(t3, "bold")], "🐟", "fish", loose);
  check(covered(r4.text, r4.runs[0]) === "bold", `past an emoji: ${covered(r4.text, r4.runs[0])}`);
  // Taken away entirely, a run over only what was taken goes with it.
  const t5 = "keep gone keep";
  const r5 = replaceIn(t5, [runOver(t5, "gone")], "gone ", "", loose);
  check(r5.text === "keep keep" && r5.runs.length === 0, `a run over what went goes: ${JSON.stringify(r5.runs)}`);
}

if (fails.length) {
  console.error(`${fails.length} of ${ran} failed:\n  ` + fails.join("\n  "));
  process.exit(1);
}
console.log(`find and replace: ${ran} checks passed`);
