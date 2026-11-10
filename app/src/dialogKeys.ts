/**
 * Whether Enter, pressed in a window, means the window's own action (Create,
 * Export, Apply). Not when it lands on a button, which answers Enter itself
 * — so Enter on a focused Cancel cancels rather than exporting or making a
 * new document in place of the one open — nor on a list being chosen from,
 * nor in a box of several lines, where Enter is a new line. Where it does,
 * the key is taken (`preventDefault`).
 */
export function enterMeansGo(e: KeyboardEvent): boolean {
  if (e.key !== "Enter" || e.isComposing) return false;
  const t = e.target;
  const go = !(
    t instanceof HTMLButtonElement ||
    t instanceof HTMLSelectElement ||
    t instanceof HTMLTextAreaElement ||
    t instanceof HTMLAnchorElement ||
    (t instanceof HTMLInputElement && (t.type === "checkbox" || t.type === "radio"))
  );
  // Taken, it is the window's alone: the window closes on it and gives
  // the focus back, and the same key went on to press whatever had it —
  // a menu row, or a Delete button pressed last.
  if (go) e.preventDefault();
  return go;
}
