// Unit tests for `src/numberText.ts` — how a number box reads commas and
// other scripts' digits.
//
//   node e2e/numberText.test.mjs
import { build } from "esbuild";
import { unlinkSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const tmp = join(here, "out", "numberText.generated.mjs");
await build({
  entryPoints: [join(here, "..", "src", "numberText.ts")],
  bundle: true,
  format: "esm",
  outfile: tmp,
  logLevel: "silent",
});
const { readWritten, settleTyped, plainDigits, commaIsDecimal } = await import("file://" + tmp);
unlinkSync(tmp);

let ran = 0;
const fails = [];
const same = (got, want, msg) => {
  ran += 1;
  if (got !== want) fails.push(`${msg}: got ${JSON.stringify(got)}, want ${JSON.stringify(want)}`);
};

// Written elsewhere, in an English browser.
same(readWritten("12,5", false), "12.5", "a decimal comma");
same(readWritten("1,920", false), "1920", "a thousands comma");
same(readWritten("1,920,000", false), "1920000", "two thousands commas");
same(readWritten("1,234.5", false), "1234.5", "thousands then point");
same(readWritten("1.234,5", false), "1234.5", "points then decimal comma");
same(readWritten("-0,25", false), "-0.25", "negative decimal comma");
same(readWritten("12,50", false), "12.50", "two digits after is a decimal");
same(readWritten("١٢٫٥".replace("٫", ","), false), "12.5", "Arabic-Indic digits");
same(readWritten("۳۰۰", false), "300", "Persian digits");
same(readWritten("42", false), "42", "plain");
// In a browser whose language writes decimals with a comma.
same(readWritten("1,920", true), "1.920", "comma is decimal there");
same(readWritten("1.234,5", true), "1234.5", "and points are thousands there");

// Typed: commas went in as points on the way.
same(settleTyped("1.920", 1, false, false), "1920", "1,920 typed");
same(settleTyped("12.5", 1, false, false), "12.5", "12,5 typed");
same(settleTyped("1.920000", 2, false, false), "1920000", "1,920,000 typed");
same(settleTyped("1.920", 1, true, false), "1.920", "a point typed as well is a point");
same(settleTyped("1.920", 1, false, true), "1.920", "in a comma-decimal language");
same(settleTyped("1.920", 0, false, false), "1.920", "no comma typed");
same(settleTyped("-3.000", 1, false, false), "-3000", "negative thousands");
same(plainDigits("٠١٢٣٤٥٦٧٨٩"), "0123456789", "every Arabic-Indic digit");
same(readWritten("١٢٫٥", false), "12.5", "Arabic's own decimal mark");
same(readWritten("١٬٢٠٠", false), "1200", "and its thousands mark");
same(readWritten("0,500", false), "0.500", "a nought before a comma is a decimal");
same(settleTyped("0.500", 1, false, false), "0.500", "typed too");
same(typeof commaIsDecimal("de-DE"), "boolean", "the language is asked");

if (fails.length) {
  console.error(`numberText: ${fails.length} of ${ran} failed`);
  for (const f of fails) console.error("  " + f);
  process.exit(1);
}
console.log(`numberText: all ${ran} passed`);
