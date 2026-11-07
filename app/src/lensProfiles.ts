/** Lens profiles from lensfun's database, for raw files that carry no
 * lens corrections of their own (a Canon's CR2 or CR3, a Nikon's NEF).
 *
 * The database is CC BY-SA 3.0, which this GPL-3.0 program cannot carry,
 * so none of it is shipped: a person adds the files they have (lensfun's
 * `data/db`, one XML file per maker and kind of camera), which are kept
 * in IndexedDB on this machine; in the desktop shell the system's own
 * copy is read as well, where there is one. The engine is given them
 * just before it opens a raw file, so every session — a new document is
 * a new one — has the same set. */

import type { WasmSession } from "./engine";
import { systemLensProfiles } from "./files";
import { isTauri } from "./nativeMenu";

/** A database file, and how many lens calibrations the engine found in
 * it (nought until it has been given one). */
export type LensFile = { name: string; text: string; lenses: number; system?: boolean };

const DB = "chitrakar-lenses";
const STORE = "profiles";

function db(): Promise<IDBDatabase | null> {
  return new Promise((resolve) => {
    try {
      const req = indexedDB.open(DB, 1);
      req.onupgradeneeded = () => req.result.createObjectStore(STORE);
      req.onsuccess = () => resolve(req.result);
      req.onerror = () => resolve(null);
    } catch {
      resolve(null);
    }
  });
}

async function kept(): Promise<LensFile[]> {
  const d = await db();
  if (!d) return [];
  return new Promise((resolve) => {
    const req = d.transaction(STORE).objectStore(STORE).getAll();
    req.onsuccess = () =>
      resolve(
        (req.result as LensFile[]).filter(
          (f) => f && typeof f.name === "string" && typeof f.text === "string",
        ),
      );
    req.onerror = () => resolve([]);
  });
}

async function write(put: LensFile | null, remove: string | null): Promise<void> {
  const d = await db();
  if (!d) return;
  await new Promise<void>((resolve, reject) => {
    const tx = d.transaction(STORE, "readwrite");
    const store = tx.objectStore(STORE);
    if (put) store.put({ name: put.name, text: put.text, lenses: put.lenses }, put.name);
    if (remove) store.delete(remove);
    tx.oncomplete = () => resolve();
    tx.onerror = tx.onabort = () => reject(tx.error);
  });
}

/** Every file, the person's first and then the system's (one of the same
 * name as a person's is theirs); read once. */
let files: Promise<LensFile[]> | null = null;
/** Bumped whenever the set changes, so a session given an older set is
 * given the new one. */
let version = 0;
const given = new WeakMap<WasmSession, number>();

function all(): Promise<LensFile[]> {
  files ??= (async () => {
    const mine = await kept();
    let system: LensFile[] = [];
    if (isTauri()) {
      try {
        system = (await systemLensProfiles()).map((f) => ({ ...f, lenses: 0, system: true }));
      } catch {
        system = [];
      }
    }
    return [...mine, ...system.filter((s) => !mine.some((m) => m.name === s.name))];
  })();
  return files;
}

/** Give a session the profiles, if it has not had this set already. */
export async function giveLensProfiles(session: WasmSession): Promise<void> {
  const list = await all();
  if (given.get(session) === version) return;
  session.clear_lens_profiles();
  for (const f of list) {
    try {
      f.lenses = session.add_lens_profiles(f.text);
    } catch {
      f.lenses = 0;
    }
  }
  given.set(session, version);
}

/** The files as the Preferences window lists them, counted by `session`. */
export async function lensFiles(session: WasmSession | null): Promise<Omit<LensFile, "text">[]> {
  if (session) await giveLensProfiles(session);
  return (await all()).map(({ name, lenses, system }) => ({ name, lenses, system }));
}

/** Add files a person picked. Each is tried on `session` first: one that
 * is not a lensfun database is refused and not kept. A file of the same
 * name replaces the one there was. */
export async function addLensFiles(
  session: WasmSession,
  picked: File[],
): Promise<{ added: number; refused: string[] }> {
  const list = await all();
  const refused: string[] = [];
  let added = 0;
  for (const file of picked) {
    const text = await file.text();
    let lenses: number;
    try {
      lenses = session.add_lens_profiles(text);
    } catch {
      refused.push(file.name);
      continue;
    }
    const entry: LensFile = { name: file.name, text, lenses };
    const at = list.findIndex((f) => f.name === file.name);
    if (at >= 0) list.splice(at, 1, entry);
    else list.unshift(entry);
    try {
      await write(entry, null);
    } catch {
      // Not kept past this visit (private browsing, a full disk), but
      // still used until the page closes.
    }
    added += 1;
  }
  version += 1;
  return { added, refused };
}

/** Forget a file the person added. The system's own are not theirs to
 * forget here. */
export async function forgetLensFile(name: string): Promise<void> {
  const list = await all();
  const at = list.findIndex((f) => f.name === name && !f.system);
  if (at < 0) return;
  list.splice(at, 1);
  try {
    await write(null, name);
  } catch {
    // Gone for this visit all the same.
  }
  version += 1;
}

/** The profiles' texts, for the develop worker's engine: the same array
 * for as long as the set is the same, so the worker is sent them again
 * only when it changes. */
let texts: { at: number; list: string[] } | null = null;
export async function lensProfileTexts(): Promise<string[]> {
  const list = await all();
  if (texts?.at !== version) texts = { at: version, list: list.map((f) => f.text) };
  return texts.list;
}
