/** The commands that answer to a key, and the keys they answer to.
 *
 * The tools' keys became the person's on the Tools page; these are the
 * other half — save, group, copy a look, send to the back — and they
 * were literals spread through two keydown handlers and a hint string
 * spelled again on every menu row. One table now, read by the handlers,
 * by the menus and by the window that rebinds them, so a key changed in
 * one place is changed everywhere it is written down.
 *
 * A chord is a string: the modifiers in a fixed order and then the key,
 * joined by `+`. `mod` is control or command, whichever the machine
 * has. Brackets are named by what is printed on them rather than by
 * what shift does to them, since the two differ by keyboard.
 */

/** Everything that can be rebound. */
export const COMMANDS = [
  { id: "new", label: "New document", group: "File", chord: "mod+n" },
  { id: "open", label: "Open…", group: "File", chord: "mod+o" },
  { id: "save", label: "Save", group: "File", chord: "mod+s" },
  // Only in the desktop shell, where a document has somewhere to be.
  { id: "save-as", label: "Save as…", group: "File", chord: "mod+shift+s" },
  { id: "export", label: "Export PNG", group: "File", chord: "mod+e" },
  { id: "export-window", label: "Export…", group: "File", chord: "mod+shift+e" },
  { id: "preferences", label: "Preferences", group: "File", chord: "mod+," },
  { id: "undo", label: "Undo", group: "Edit", chord: "mod+z" },
  { id: "redo", label: "Redo", group: "Edit", chord: "mod+shift+z" },
  { id: "cut", label: "Cut", group: "Edit", chord: "mod+x" },
  { id: "copy", label: "Copy", group: "Edit", chord: "mod+c" },
  { id: "paste", label: "Paste", group: "Edit", chord: "mod+v" },
  // The one that fires while a field has the caret, as it always has.
  { id: "duplicate", label: "Duplicate", group: "Edit", chord: "mod+d", typing: true },
  { id: "copy-style", label: "Copy style", group: "Edit", chord: "mod+alt+c" },
  { id: "paste-style", label: "Paste style", group: "Edit", chord: "mod+alt+v" },
  { id: "find-replace", label: "Find and replace…", group: "Edit", chord: "mod+f" },
  { id: "select-all", label: "Select all", group: "Select", chord: "mod+a" },
  { id: "pick-inverse", label: "Pick out the rest instead", group: "Select", chord: "mod+shift+i" },
  { id: "group", label: "Group", group: "Layer", chord: "mod+g" },
  { id: "ungroup", label: "Ungroup", group: "Layer", chord: "mod+shift+g" },
  { id: "clip", label: "Clip to the layer below", group: "Layer", chord: "mod+alt+g" },
  { id: "to-front", label: "Bring to front", group: "Layer", chord: "mod+shift+]" },
  { id: "to-back", label: "Send to back", group: "Layer", chord: "mod+shift+[" },
  { id: "zoom-in", label: "Zoom in", group: "View", chord: "mod+=" },
  { id: "zoom-out", label: "Zoom out", group: "View", chord: "mod+-" },
  { id: "zoom-fit", label: "Fit document to window", group: "View", chord: "mod+0" },
  { id: "zoom-actual", label: "Actual size", group: "View", chord: "mod+1" },
  { id: "keys-sheet", label: "Keys and gestures", group: "Help", chord: "shift+/" },
] as const;

export type CommandId = (typeof COMMANDS)[number]["id"];
export type CommandGroup = (typeof COMMANDS)[number]["group"];

export const COMMAND_GROUPS: readonly CommandGroup[] = ["File", "Edit", "Select", "Layer", "View", "Help"];

/** A person's rebindings: the chord each named command answers to
 * instead of the one it shipped with. An empty string is a command
 * given no chord at all — asked for, not a chord that failed to read. */
export type CommandKeys = Partial<Record<CommandId, string>>;

/** The chords a person has given to menu rows that ship with none:
 * any row of any menu, by the row's id — the same id the desktop's
 * system menu answers with, so it outlives the render that wrote it. */
export type MenuKeys = Record<string, string>;

/** Whether a chord may be given to a command or a menu row. A letter
 * or a digit pressed alone is a tool's — the Tools page hands those
 * out — and with only shift it walks a tool's family; a bracket alone
 * sizes the brush. Anything else is free to take. */
export function claimable(chord: string): boolean {
  if (!isChord(chord)) return false;
  const parts = chord.split("+");
  const key = parts.pop()!;
  if (parts.includes("mod") || parts.includes("alt")) return true;
  return !/^[a-z0-9[\]]$/.test(key);
}

const isCommandId = (s: unknown): s is CommandId =>
  typeof s === "string" && COMMANDS.some((c) => c.id === s);

/** Whether a string is a chord this can match and show. One key, the
 * modifiers in order, nothing else. */
export function isChord(s: unknown): s is string {
  if (typeof s !== "string") return false;
  const parts = s.split("+");
  const key = parts.pop();
  if (!key || key.length !== 1) return false;
  const seen = new Set<string>();
  for (const p of parts) {
    if (!["mod", "alt", "shift"].includes(p) || seen.has(p)) return false;
    seen.add(p);
  }
  // In the order they are written, so one chord is one string.
  return parts.join("+") === ["mod", "alt", "shift"].filter((m) => seen.has(m)).join("+");
}

/** The chord a key press makes, in the same spelling the table uses. */
export function chordFromEvent(e: KeyboardEvent): string {
  const parts: string[] = [];
  if (e.ctrlKey || e.metaKey) parts.push("mod");
  if (e.altKey) parts.push("alt");
  if (e.shiftKey) parts.push("shift");
  // What is printed on the key, not what shift makes of it: a keyboard
  // that gives `}` for shift+`]` would otherwise be a different chord
  // from one that gives `]`.
  const printed: Record<string, string> = {
    BracketLeft: "[",
    BracketRight: "]",
    Comma: ",",
    Period: ".",
    Slash: "/",
    Semicolon: ";",
    Quote: "'",
    Backslash: "\\",
    Minus: "-",
    Equal: "=",
  };
  const key = printed[e.code] ?? (e.key.length === 1 ? e.key.toLowerCase() : "");
  if (!key) return "";
  parts.push(key);
  return parts.join("+");
}

/** A chord as a person reads it. `mod` is written as the machine's own
 * name for it, since that is what is printed on their keyboard. */
export function chordLabel(chord: string, mac = onAMac()): string {
  const parts = chord.split("+");
  const key = parts.pop() ?? "";
  const named: Record<string, string> = { mod: mac ? "Cmd" : "Ctrl", alt: mac ? "Opt" : "Alt", shift: "Shift" };
  return [...parts.map((p) => named[p] ?? p), key.toUpperCase()].join("+");
}

export function onAMac(): boolean {
  if (typeof navigator === "undefined") return false;
  return /mac|iphone|ipad/i.test(navigator.platform || navigator.userAgent || "");
}

/** The chords as they stand, a person's rebindings over the defaults:
 * which command each chord fires, and what each command's chord is. A
 * command whose default chord a rebinding took is left with none, and
 * says so with an empty string. */
export function boundChords(
  rebound: CommandKeys,
  menuKeys: MenuKeys = {},
): {
  byChord: Record<string, CommandId>;
  of: Record<CommandId, string>;
  /** Which menu row each chord a person gave one runs. */
  menuByChord: Record<string, string>;
  /** And the chord of each menu row that has one. */
  menuOf: Record<string, string>;
} {
  const held = new Map<CommandId, string>();
  for (const c of COMMANDS) held.set(c.id, c.chord);
  for (const [id, chord] of Object.entries(rebound)) {
    if (isCommandId(id) && (isChord(chord) || chord === "")) held.set(id, chord);
  }
  // Defaults first and rebindings after, so a rebinding takes its chord
  // from whoever held it.
  const order = [...held.keys()].sort((a, b) => Number(a in rebound) - Number(b in rebound));
  const byChord: Record<string, CommandId> = {};
  const of: Partial<Record<CommandId, string>> = {};
  for (const id of order) {
    const chord = held.get(id)!;
    of[id] = chord;
    if (!chord) continue;
    const loser = byChord[chord];
    if (loser) of[loser] = "";
    byChord[chord] = id;
  }
  // A menu row's chord comes last of all: it was given on purpose,
  // over whatever command held it.
  const menuByChord: Record<string, string> = {};
  const menuOf: Record<string, string> = {};
  for (const [row, chord] of Object.entries(menuKeys)) {
    if (!claimable(chord)) continue;
    const loser = byChord[chord];
    if (loser) {
      of[loser] = "";
      delete byChord[chord];
    }
    const rowLoser = menuByChord[chord];
    if (rowLoser) delete menuOf[rowLoser];
    menuByChord[chord] = row;
    menuOf[row] = chord;
  }
  return { byChord, of: of as Record<CommandId, string>, menuByChord, menuOf };
}

/** Give `chord` to a command or a menu row, as the Keys page does. A
 * chord someone else holds changes hands: the one who held it takes
 * the chord the taker had, so a swap leaves nothing unreachable, or is
 * left with none. An empty chord takes the target's away. Rebindings
 * that only say a default again are not kept. */
export function giveChord(
  target: { command: CommandId } | { row: string },
  chord: string,
  rebound: CommandKeys,
  menuKeys: MenuKeys,
): { commandKeys: CommandKeys; menuKeys: MenuKeys } {
  const now = boundChords(rebound, menuKeys);
  const commandKeys: CommandKeys = { ...rebound };
  const rows: MenuKeys = { ...menuKeys };
  const had = "command" in target ? now.of[target.command] : (now.menuOf[target.row] ?? "");
  if (chord) {
    const holder = now.byChord[chord];
    const rowHolder = now.menuByChord[chord];
    if (holder && !("command" in target && target.command === holder)) {
      commandKeys[holder] = had && claimable(had) ? had : "";
    }
    if (rowHolder && !("row" in target && target.row === rowHolder)) {
      if (had && claimable(had)) rows[rowHolder] = had;
      else delete rows[rowHolder];
    }
  }
  if ("command" in target) commandKeys[target.command] = chord;
  else if (chord) rows[target.row] = chord;
  else delete rows[target.row];
  for (const id of Object.keys(commandKeys) as CommandId[]) {
    const shipped = COMMANDS.find((c) => c.id === id)?.chord;
    if (commandKeys[id] === shipped) delete commandKeys[id];
  }
  return { commandKeys, menuKeys: rows };
}

/** Put a command back to the chord it shipped with, taking that chord
 * back from whoever holds it now. */
export function restoreChord(
  command: CommandId,
  rebound: CommandKeys,
  menuKeys: MenuKeys,
): { commandKeys: CommandKeys; menuKeys: MenuKeys } {
  const shipped = COMMANDS.find((c) => c.id === command)!.chord;
  const commandKeys: CommandKeys = { ...rebound };
  const rows: MenuKeys = { ...menuKeys };
  delete commandKeys[command];
  for (const id of Object.keys(commandKeys) as CommandId[]) {
    if (commandKeys[id] === shipped) delete commandKeys[id];
  }
  for (const row of Object.keys(rows)) if (rows[row] === shipped) delete rows[row];
  return { commandKeys, menuKeys: rows };
}

/** Whether a command fires while the caret is in a field. */
export const firesWhileTyping = (id: CommandId): boolean =>
  COMMANDS.some((c) => c.id === id && "typing" in c && c.typing);
