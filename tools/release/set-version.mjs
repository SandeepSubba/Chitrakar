// Sets the version everywhere it is written, in one step:
//
//   node tools/release/set-version.mjs 1.0.0
//
// The app's package.json and its lock, the shell's tauri.conf.json (what
// the installers and the About window say), the workspace's Cargo.toml
// (every engine crate takes it from there) and the shell's own
// Cargo.toml. Cargo.lock follows on the next build. Then tag `v1.0.0`.
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const version = process.argv[2];
if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version ?? "")) {
  console.error("usage: node tools/release/set-version.mjs <major.minor.patch[-pre]>");
  process.exit(1);
}
const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const edit = (path, change) => {
  const file = join(root, path);
  const before = readFileSync(file, "utf8");
  const after = change(before);
  if (after === before) {
    console.error(`set-version: nothing to change in ${path} (is the version already ${version}?)`);
    return;
  }
  writeFileSync(file, after);
  console.log(`${path}: ${version}`);
};
const json = (path, at) =>
  edit(path, (text) => {
    const doc = JSON.parse(text);
    for (const set of at) set(doc);
    return JSON.stringify(doc, null, 2) + "\n";
  });

json("app/package.json", [(d) => (d.version = version)]);
json("app/package-lock.json", [
  (d) => (d.version = version),
  (d) => d.packages?.[""] && (d.packages[""].version = version),
]);
json("shells/tauri/src-tauri/tauri.conf.json", [(d) => (d.version = version)]);
// The first `version = ` under [workspace.package], and the shell's own.
edit("Cargo.toml", (t) => t.replace(/(\[workspace\.package\][^[]*?\nversion = ")[^"]*(")/, `$1${version}$2`));
edit("shells/tauri/src-tauri/Cargo.toml", (t) => t.replace(/(\[package\][^[]*?\nversion = ")[^"]*(")/, `$1${version}$2`));
