/** Files, the way the app is running.
 *
 * In a browser a file is written by being downloaded and read by being
 * chosen in an `<input>`: the page never learns where anything is. In
 * the desktop shell the system's own panels answer, a document knows
 * the path it came from, and Save writes back over it. This is the one
 * place that knows which of the two is happening; the commands it calls
 * are in `shells/tauri/src-tauri/src/lib.rs`.
 */

import { isTauri, onMobile } from "./nativeMenu";

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
  psd: { name: "Photoshop document", extensions: ["psd"] },
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

/** A path's parts, folders first. An address the system gave rather
 * than a path — Android's `content://…/document/primary%3ADownload%2Fa.png`
 * — carries its path encoded in its last segment, after the volume's
 * name and a colon, so that is where the parts are read from. */
const partsOf = (path: string): string[] => {
  // A file: address — what an iPhone's picker gives, kept as one so the
  // shell can open it under the picker's grant — is a path, encoded.
  if (/^file:\/\//i.test(path)) {
    let inner = path.replace(/^file:\/\/[^/]*/i, "");
    try {
      inner = decodeURIComponent(inner);
    } catch {
      // Not encoded after all: read as it is.
    }
    return inner.split("/").filter(Boolean);
  }
  if (/^[a-z][a-z0-9+.-]+:\/\//i.test(path) && !/^file:/i.test(path)) {
    const last = path.split("/").filter(Boolean).pop() ?? path;
    let inner = last;
    try {
      inner = decodeURIComponent(last);
    } catch {
      // Not encoded after all: read as it is.
    }
    return inner.replace(/^[^:/]*:/, "").split("/").filter(Boolean);
  }
  return path.split(/[\\/]/).filter(Boolean);
};

/** A path's last part: what a person calls the file. */
export const baseName = (path: string) => partsOf(path).pop() ?? path;

/** The folder a path is in, by its own last part — enough to tell two
 * files of the same name apart on a menu. */
export const folderName = (path: string) => {
  const parts = partsOf(path);
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

/** A name every system will give a file: the characters a path or
 * Windows will not take (a slash would make a folder of a region called
 * "icon/small", a colon is refused outright) become "-", control
 * characters go, and so do the dots and spaces Windows drops off the
 * end. Never empty. */
/** A string as the characters a person sees: an emoji with its skin
 * tone, a letter with its accent — so a name cut short is not cut
 * through one. Code points where the browser cannot say. */
export function graphemes(text: string): string[] {
  const Seg = (Intl as unknown as { Segmenter?: new (l?: string, o?: { granularity: string }) => { segment(t: string): Iterable<{ segment: string }> } }).Segmenter;
  return Seg ? [...new Seg(undefined, { granularity: "grapheme" }).segment(text)].map((g) => g.segment) : [...text];
}

export function fileSafe(name: string): string {
  let cleaned = name
    // eslint-disable-next-line no-control-regex
    .replace(/[\u0000-\u001f\u007f]/g, "")
    .replace(/[\\/:*?"<>|]/g, "-")
    .replace(/[. ]+$/, "")
    .trim();
  // Most systems refuse a name past 255 bytes — bytes, and a character
  // of Chinese is three of them: a long region name is cut to fit,
  // keeping what it ends with (`@2x.png`, `-1000.jpg`).
  const MOST = 120;
  const bytes = (t: string) => new TextEncoder().encode(t).length;
  if (graphemes(cleaned).length > MOST || bytes(cleaned) > 240) {
    const dot = cleaned.lastIndexOf(".");
    const end = dot > 0 && cleaned.length - dot <= 16 ? cleaned.slice(dot) : "";
    const tail = cleaned.slice(0, cleaned.length - end.length).match(/(@\d+(\.\d+)?x|-\d+)$/)?.[0] ?? "";
    let stem = graphemes(cleaned.slice(0, cleaned.length - end.length - tail.length));
    stem = stem.slice(0, MOST - [...tail].length - [...end].length);
    while (stem.length > 0 && bytes(stem.join("") + tail + end) > 240) stem.pop();
    cleaned = stem.join("").trim() + tail + end;
  }
  return cleaned || "untitled";
}

/** Names made distinct, so files written together do not land on one
 * another — two frames both called "Hero" — and in a way that holds
 * where case does not count: a second "Hero.png" is "Hero 2.png". */
export function distinctNames(names: string[]): string[] {
  const taken = new Set<string>();
  return names.map((name) => {
    const dot = name.lastIndexOf(".");
    const [stem, ext] = dot > 0 ? [name.slice(0, dot), name.slice(dot)] : [name, ""];
    let out = name;
    for (let n = 2; taken.has(out.toLowerCase()); n++) out = `${stem} ${n}${ext}`;
    taken.add(out.toLowerCase());
    return out;
  });
}

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
  // Named from what the person called a frame, a region or a comp, so
  // made into names a file can have, and none of them twice.
  const named = distinctNames(files.map((f) => fileSafe(f.name)));
  files = files.map((f, i) => ({ ...f, name: named[i] }));
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
  // A phone has no folders to choose. Where it can share, the files go
  // to its share sheet together — Save to Files is one of the answers
  // there — rather than to a question it has no panel for.
  if (onMobile() && canShareFiles()) return shareFiles(title, files);
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

/** lensfun's lens database where the system keeps one (see
 * `lensProfiles.ts`): each XML file by its name. */
export const systemLensProfiles = () =>
  call<{ name: string; text: string }[]>("system_lens_profiles");

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
    avif: "image/avif",
    heic: "image/heic",
    heif: "image/heif",
    psd: "image/vnd.adobe.photoshop",
    pdf: "application/pdf",
  };
  return types[ext] ?? null;
}

/** Whether this device can hand files to its share sheet — a phone, a
 * tablet, Safari, Chrome on Windows. Asked of a one-pixel PNG, since a
 * device that shares any file shares a picture. */
export function canShareFiles(): boolean {
  try {
    return (
      typeof navigator !== "undefined" &&
      typeof navigator.canShare === "function" &&
      navigator.canShare({ files: [new File([new Uint8Array(1)], "probe.png", { type: "image/png" })] })
    );
  } catch {
    return false;
  }
}

/** Hand files to the system's share sheet: where they go then is the
 * person's to say — Files, a message, another app, AirDrop. Answers
 * whether they went; false is the sheet put away. A format the sheet
 * will not take is said, naming it. */
export async function shareFiles(title: string, files: Outgoing[]): Promise<boolean> {
  const made = files.map((f) => new File([f.make() as BlobPart], f.name, { type: f.type }));
  if (!navigator.canShare?.({ files: made })) {
    const kinds = [...new Set(made.map((f) => f.type || "unknown"))].join(", ");
    throw new Error(`this device cannot share ${made.length === 1 ? "this kind of file" : "these files"} (${kinds})`);
  }
  try {
    await navigator.share({ files: made, title });
    return true;
  } catch (err) {
    if (err instanceof DOMException && err.name === "AbortError") return false;
    throw err;
  }
}
