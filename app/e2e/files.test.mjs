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
const { baseName, folderName, replacing } = await import("file://" + tmp);
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
// A file: address is not one of these.
check(baseName("file:///tmp/a.png") === "a.png", "a file: address is a path");
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
