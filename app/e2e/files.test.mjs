// Unit tests for the naming half of `src/files.ts`: what a person calls
// a file the system handed over, by a path or — on Android — by a
// `content://` address that carries its path encoded in its last
// segment. Run without a browser, transpiled the way the runs test is,
// with the shell's own check stood in for since none of this asks it.
//
//   node e2e/files.test.mjs
import { transform } from "esbuild";
import { readFileSync, writeFileSync, unlinkSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const src = readFileSync(join(here, "..", "src", "files.ts"), "utf8").replace(
  /import \{[^}]*\} from "\.\/nativeMenu";/,
  "const isTauri = () => false, onMobile = () => false;",
);
const js = (await transform(src, { loader: "ts", format: "esm" })).code;
const tmp = join(here, "out", "files.generated.mjs");
writeFileSync(tmp, js);
const { baseName, folderName, replacing, fileSafe, distinctNames, graphemes } = await import("file://" + tmp);
unlinkSync(tmp);

let ran = 0;
const fails = [];
const check = (cond, msg) => {
  ran += 1;
  if (!cond) fails.push(msg);
};
const names = (p, base, folder) => {
  check(baseName(p) === base, `${p}: called ${JSON.stringify(baseName(p))}, not ${JSON.stringify(base)}`);
  check(folderName(p) === folder, `${p}: in ${JSON.stringify(folderName(p))}, not ${JSON.stringify(folder)}`);
};

names("/home/me/a b.chitra", "a b.chitra", "me");
names("C:\\Users\\me\\Pictures\\x.png", "x.png", "Pictures");
names("x.png", "x.png", "");
// Android's own storage: the volume, a colon, and the path, encoded.
names(
  "content://com.android.externalstorage.documents/document/primary%3ADownload%2FHolidays%2Fpic.png",
  "pic.png",
  "Holidays",
);
// Downloads by number: no name to give, so the number it has.
names("content://com.android.providers.downloads.documents/document/msf%3A1000", "1000", "");
// Not encoded after all, or badly: read as it is, never thrown.
names("content://some.provider/files/pic.png", "pic.png", "");
names("content://some.provider/files/100%25%ZZ", "100%25%ZZ", "");
// A file: address — an iPhone's, kept for its grant — is a path, encoded.
check(baseName("file:///tmp/a.png") === "a.png", "a file: address is a path");

// A region, comp or frame's name is a file's name, so it is made one
// every system takes: no folders out of a slash, no colon for Windows
// to refuse, no trailing dot it would drop.
check(
  fileSafe("untitled-icon/small: v2@1x.png") === "untitled-icon-small- v2@1x.png",
  `a slash and a colon are not kept (${fileSafe("untitled-icon/small: v2@1x.png")})`,
);
check(fileSafe("a\u000bb.png") === "ab.png", "control characters go");
check(fileSafe("Hero. ") === "Hero", "nor do the dots and spaces Windows drops");
check(fileSafe("///") === "---" && fileSafe("") === "untitled", "and it is never empty");
const wide = fileSafe(`${"漢".repeat(110)}@2x.png`);
check(new TextEncoder().encode(wide).length <= 240 && wide.endsWith("@2x.png"), `cut by its bytes (${new TextEncoder().encode(wide).length})`);
const thumbs = fileSafe(`${"👍🏽".repeat(130)}.png`);
check(graphemes(thumbs.slice(0, -4)).every((g) => g === "👍🏽"), "a cut never splits a skin tone from its thumb");
const dotted = fileSafe(`${"a".repeat(119)}. more words here`);
check(!dotted.endsWith(".") && !dotted.endsWith(" "), `a cut name does not end on a dot (${dotted.slice(-6)})`);
const edge = fileSafe(`untitled-${"r".repeat(300)}-1000.jpg`);
check(edge.endsWith("-1000.jpg"), `a long edge's size is kept (${edge.slice(-12)})`);
const long = fileSafe(`untitled-${"r".repeat(300)}@2x.png`);
check(
  [...long].length <= 120 && long.endsWith("@2x.png") && long.startsWith("untitled-rrr"),
  `a long name is cut, keeping how it ends (${long.length}: ${long.slice(-12)})`,
);
// Files written together never land on one another, case or no case.
const distinct = distinctNames(["Hero.png", "hero.png", "Hero.png", "Other.png"]);
check(
  JSON.stringify(distinct) === JSON.stringify(["Hero.png", "hero 2.png", "Hero 3.png", "Other.png"]),
  `two frames of one name are two files (${distinct})`,
);
names("file:///private/var/mobile/Holiday%20snaps/sea%20view.png", "sea view.png", "Holiday snaps");
// The question before writing over files names them the same way.
check(
  replacing(["content://p/document/primary%3ADownload%2Fa.png"]) === "a.png is already in Download. Replace it?",
  replacing(["content://p/document/primary%3ADownload%2Fa.png"]),
);

if (fails.length) {
  console.error(`files: ${fails.length} of ${ran} failed`);
  for (const f of fails) console.error("  " + f);
  process.exit(1);
}
console.log(`files: ${ran} checks passed`);
