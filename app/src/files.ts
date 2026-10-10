/** Files, the way the app is running.
 *
 * In a browser a file is written by being downloaded and read by being
 * chosen in an `<input>`: the page never learns where anything is. In
 * the desktop shell the system's own panels answer, a document knows
 * the path it came from, and Save writes back over it. This is the one
 * place that knows which of the two is happening; the commands it calls
 * are in `shells/tauri/src-tauri/src/lib.rs`.
 */

import { isTauri } from "./nativeMenu";

/** One line of a panel's "files of type" list. */
export type Filter = { name: string; extensions: string[] };

const KINDS: Record<string, Filter> = {
  chitra: { name: "Chitrakar document", extensions: ["chitra"] },
  png: { name: "PNG image", extensions: ["png"] },
  jpg: { name: "JPEG image", extensions: ["jpg", "jpeg"] },
  jpeg: { name: "JPEG image", extensions: ["jpg", "jpeg"] },
  webp: { name: "WebP image", extensions: ["webp"] },
  pdf: { name: "PDF document", extensions: ["pdf"] },
  svg: { name: "SVG drawing", extensions: ["svg"] },
  tif: { name: "TIFF image", extensions: ["tif", "tiff"] },
  tiff: { name: "TIFF image", extensions: ["tiff", "tif"] },
};

/** The panel filter for a file of this name, by its extension — and the
 * extension it was given first, so the panel puts back the one the name
 * already has. */
export function filtersFor(name: string): Filter[] {
  const ext = /\.([a-z0-9]+)$/i.exec(name)?.[1]?.toLowerCase();
  const kind = ext ? KINDS[ext] : undefined;
  if (!kind) return [];
  return [{ ...kind, extensions: [ext!, ...kind.extensions.filter((e) => e !== ext)] }];
}

export const DOCUMENTS: Filter[] = [KINDS.chitra];

/** A path's last part: what a person calls the file. */
export const baseName = (path: string) => path.split(/[\\/]/).filter(Boolean).pop() ?? path;

/** The folder a path is in, by its own last part — enough to tell two
 * files of the same name apart on a menu. */
export const folderName = (path: string) => {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts.length > 1 ? parts[parts.length - 2] : "";
};

async function call<T>(
  cmd: string,
  args?: Record<string, unknown> | Uint8Array,
  options?: { headers: Record<string, string> },
): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args, options);
}

/** The system's open panel. `null` is the panel cancelled. */
export const chooseToOpen = (title: string, filters: Filter[]) =>
  call<string | null>("choose_to_open", { title, filters });

/** The system's save panel, offering `name` in the folder `beside` is
 * in when there is one. `null` is the panel cancelled. */
export const chooseToSave = (
  title: string,
  name: string,
  beside: string | null,
  filters: Filter[],
) => call<string | null>("choose_to_save", { title, name, beside, filters });

/** A file's bytes by path. */
export const readPath = async (path: string) =>
  new Uint8Array(await call<ArrayBuffer>("read_path", { path }));

/** Write a file by path, whole or not at all. The bytes cross as bytes
 * and the path beside them in a header, which has to be ASCII. */
export const writePath = (path: string, bytes: Uint8Array) =>
  call<void>("write_path", bytes, { headers: { path: encodeURIComponent(path) } });

/** One file to write: its name, its type, and how to make it — made
 * only once it is known where it is going, so a cancelled panel costs
 * no encode. */
export type Outgoing = { name: string; type: string; make: () => Uint8Array };

/** Write files out, as the platform writes them: downloads in a
 * browser; in the shell a save panel for one file and a folder for
 * several, since a panel per file is not a thing anybody wants to
 * answer twelve times. Starts beside `near` where there is one. Answers
 * whether they were written — false is a panel cancelled; a failure
 * throws, naming it. */
export async function writeFiles(
  title: string,
  files: Outgoing[],
  near: string | null,
): Promise<boolean> {
  if (files.length === 0) return false;
  if (!isTauri()) {
    for (const f of files) download(f.make(), f.name, f.type);
    return true;
  }
  if (files.length === 1) {
    const [f] = files;
    const path = await chooseToSave(title, f.name, near, filtersFor(f.name));
    if (!path) return false;
    await writePath(path, f.make());
    return true;
  }
  const folder = await call<string | null>("choose_folder", { title });
  if (!folder) return false;
  const paths = await Promise.all(
    files.map((f) => call<string>("join_path", { folder, name: f.name })),
  );
  // A save panel asks before writing over a file; a folder has no
  // panel to ask, so this does — once, for all of them.
  const there = await call<string[]>("already_there", { paths });
  if (there.length > 0 && !window.confirm(replacing(there))) return false;
  for (const [i, f] of files.entries()) await writePath(paths[i], f.make());
  return true;
}

/** The files the system has asked the app to open — a document
 * double-clicked, a picture sent with "Open with" — that nothing has
 * opened yet. Each is handed out once. */
export const openedFiles = () => call<string[]>("opened_files");

/** The question asked before writing over files already in a folder. */
export function replacing(there: string[]): string {
  const names = there.map(baseName);
  const listed =
    names.length <= 3 ? names.join(", ") : `${names.slice(0, 3).join(", ")} and ${names.length - 3} more`;
  return there.length === 1
    ? `${names[0]} is already in ${folderName(there[0])}. Replace it?`
    : `${there.length} of these files are already in ${folderName(there[0])} (${listed}). Replace them?`;
}

function download(bytes: Uint8Array, name: string, type: string) {
  const url = URL.createObjectURL(new Blob([bytes as BlobPart], { type }));
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.click();
  URL.revokeObjectURL(url);
}

/** What a picture is, by its name — for a file arriving by path, which
 * carries no type the way a browser's `File` does. `null` for anything
 * that is not a picture, which a drop then passes over the way a
 * browser's drop passes over what is not `image/*`. */
export function imageType(name: string): string | null {
  const ext = /\.([a-z0-9]+)$/i.exec(name)?.[1]?.toLowerCase() ?? "";
  const types: Record<string, string> = {
    png: "image/png",
    jpg: "image/jpeg",
    jpeg: "image/jpeg",
    svg: "image/svg+xml",
    gif: "image/gif",
    webp: "image/webp",
    tif: "image/tiff",
    tiff: "image/tiff",
    bmp: "image/bmp",
  };
  return types[ext] ?? null;
}
