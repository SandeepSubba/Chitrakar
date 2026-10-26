// Unit tests for `src/commands.ts` — which chord runs which command or
// menu row, and how a chord changes hands when it is given away. Asked
// without a browser.
//
//   node e2e/commands.test.mjs
import { build } from "esbuild";
import { unlinkSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const tmp = join(here, "out", "commands.generated.mjs");
await build({
  entryPoints: [join(here, "..", "src", "commands.ts")],
  bundle: true,
  format: "esm",
  outfile: tmp,
  logLevel: "silent",
});
const { COMMANDS, boundChords, claimable, giveChord, restoreChord } = await import("file://" + tmp);
unlinkSync(tmp);

let ran = 0;
const fails = [];
const check = (cond, msg) => {
  ran += 1;
  if (!cond) fails.push(msg);
};

// What may be given: a letter alone is a tool's, with shift a tool
// family's, a bracket alone the brush's; with ctrl or alt anything is.
check(!claimable("k"), "a letter alone is a tool's");
check(!claimable("shift+k"), "shift and a letter walk a tool family");
check(!claimable("]"), "a bracket alone sizes the brush");
check(claimable("mod+k") && claimable("alt+k") && claimable("mod+alt+shift+7"), "with ctrl or alt, anything");
check(claimable("shift+/") && claimable("/"), "punctuation alone is free");
check(!claimable("shift+mod+k"), "modifiers out of order are not a chord");

// Every command has a chord nobody else ships with.
{
  const b = boundChords({});
  const chords = COMMANDS.map((c) => c.chord);
  check(new Set(chords).size === chords.length, "no two commands ship with one chord");
  check(COMMANDS.every((c) => b.of[c.id] === c.chord && b.byChord[c.chord] === c.id), "the defaults stand");
}

// A menu row given a free chord is run by it, and says so.
{
  const g = giveChord({ row: "flip-h" }, "mod+alt+h", {}, {});
  const b = boundChords(g.commandKeys, g.menuKeys);
  check(b.menuByChord["mod+alt+h"] === "flip-h" && b.menuOf["flip-h"] === "mod+alt+h", "a row takes a free chord");
  check(Object.keys(g.commandKeys).length === 0, "and no command is touched");
}

// A menu row given a command's chord takes it; the command is left with
// none, since the row had none to give back.
{
  const g = giveChord({ row: "flip-h" }, "mod+g", {}, {});
  const b = boundChords(g.commandKeys, g.menuKeys);
  check(b.menuByChord["mod+g"] === "flip-h", "the row has ctrl+g");
  check(b.of.group === "" && b.byChord["mod+g"] === undefined, `group has none (${JSON.stringify(b.of.group)})`);
  // Put back, group takes its chord back from the row.
  const r = restoreChord("group", g.commandKeys, g.menuKeys);
  const b2 = boundChords(r.commandKeys, r.menuKeys);
  check(b2.of.group === "mod+g" && b2.menuOf["flip-h"] === undefined, "restored, group has it again and the row none");
}

// Two commands swap: save takes group's chord, group takes save's.
{
  const g = giveChord({ command: "save" }, "mod+g", {}, {});
  const b = boundChords(g.commandKeys, g.menuKeys);
  check(b.of.save === "mod+g" && b.of.group === "mod+s", `a swap (${b.of.save}, ${b.of.group})`);
}

// A row that had a chord swaps it with the row it takes from.
{
  let k = giveChord({ row: "a" }, "mod+alt+1", {}, {});
  k = giveChord({ row: "b" }, "mod+alt+2", k.commandKeys, k.menuKeys);
  k = giveChord({ row: "a" }, "mod+alt+2", k.commandKeys, k.menuKeys);
  const b = boundChords(k.commandKeys, k.menuKeys);
  check(b.menuOf.a === "mod+alt+2" && b.menuOf.b === "mod+alt+1", `rows swap (${JSON.stringify(b.menuOf)})`);
}

// An empty chord takes one away: from a command, which keeps that
// through a reload as an empty string, and from a row.
{
  let k = giveChord({ command: "duplicate" }, "", {}, {});
  check(k.commandKeys.duplicate === "", "a command given none says so");
  const b = boundChords(k.commandKeys, k.menuKeys);
  check(b.of.duplicate === "" && b.byChord["mod+d"] === undefined, "and nothing runs it");
  k = giveChord({ row: "a" }, "mod+alt+9", k.commandKeys, k.menuKeys);
  k = giveChord({ row: "a" }, "", k.commandKeys, k.menuKeys);
  check(!("a" in k.menuKeys), "a row given none is forgotten");
}

// A rebinding that only says the default again is not kept.
{
  let k = giveChord({ command: "save" }, "mod+alt+s", {}, {});
  k = giveChord({ command: "save" }, "mod+s", k.commandKeys, k.menuKeys);
  check(Object.keys(k.commandKeys).length === 0, `back at the default, nothing is kept (${JSON.stringify(k.commandKeys)})`);
}

// Rows' chords that could not be given are passed over on the way in.
{
  const b = boundChords({}, { a: "k", b: "mod+alt+k" });
  check(b.menuOf.a === undefined && b.menuOf.b === "mod+alt+k", "a stored chord a tool owns is passed over");
}

if (fails.length) {
  for (const f of fails) console.error("FAIL:", f);
  process.exit(1);
}
console.log(`commands: ${ran} checks passed`);
