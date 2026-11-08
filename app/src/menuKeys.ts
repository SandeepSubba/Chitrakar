/** The keys a menu answers to, the ones `role="menu"` promises a screen
 * reader and every desktop menu has: Up and Down walk the rows (round
 * from the last to the first), Home and End go to either end, a letter
 * goes to the next row it starts, Right opens the menu behind a row and
 * Left or Escape closes it again, back on the row. In the bar, Left and
 * Right go to the neighbouring menu. One function for every menu — the
 * bar's, a menu inside one, the canvas's own — so they cannot drift.
 */

/** The rows of `pop` itself, not those of a menu open inside it. */
export function rowsOf(pop: Element): HTMLElement[] {
  return [...pop.querySelectorAll<HTMLElement>("[role=menuitem]")].filter(
    (row) => row.closest("[role=menu]") === pop && !(row as HTMLButtonElement).disabled,
  );
}

/** Put the focus on a menu's first row once it is on the page: a menu
 * opened in this frame is drawn in the next. */
export function focusFirstRow(find: () => Element | null | undefined): void {
  requestAnimationFrame(() => {
    const pop = find();
    if (pop) rowsOf(pop)[0]?.focus({ preventScroll: true });
  });
}

/** The keys inside an open menu. `close` puts the menu away (the bar's
 * menu, or the canvas's); `across` moves to the bar's neighbouring menu
 * by -1 or 1, where there is a bar. Returns whether the key was the
 * menu's, in which case nothing else should hear it. */
export function menuKeyDown(
  e: KeyboardEvent,
  pop: HTMLElement,
  close: () => void,
  across?: (by: -1 | 1) => void,
): boolean {
  // A key that reached this menu from one open inside it was that
  // menu's to answer.
  const here = (e.target as Element | null)?.closest?.("[role=menu]");
  if (here && here !== pop) return false;
  const rows = rowsOf(pop);
  const at = rows.indexOf(document.activeElement as HTMLElement);
  const go = (i: number) => rows[(i + rows.length) % rows.length]?.focus({ preventScroll: true });
  const sub = pop.classList.contains("sub");
  const parentRow = () =>
    pop.parentElement?.querySelector<HTMLElement>(":scope > [aria-haspopup=menu]") ?? null;
  switch (e.key) {
    case "ArrowDown":
      go(at + 1);
      break;
    case "ArrowUp":
      go(at < 0 ? rows.length - 1 : at - 1);
      break;
    case "Home":
      go(0);
      break;
    case "End":
      go(rows.length - 1);
      break;
    case "ArrowRight": {
      const row = rows[at];
      if (row?.getAttribute("aria-haspopup") === "menu") {
        if (row.getAttribute("aria-expanded") !== "true") row.click();
        focusFirstRow(() => row.parentElement?.querySelector(":scope > [role=menu]"));
      } else if (across) {
        across(1);
      } else {
        return false;
      }
      break;
    }
    case "ArrowLeft":
      if (sub) {
        const row = parentRow();
        row?.click();
        row?.focus({ preventScroll: true });
      } else if (across) {
        across(-1);
      } else {
        return false;
      }
      break;
    case "Escape":
      if (sub) {
        const row = parentRow();
        row?.click();
        row?.focus({ preventScroll: true });
      } else {
        close();
      }
      break;
    case "Tab":
      // Tab leaves the menu for the page, and the menu goes with it.
      close();
      return false;
    default: {
      // A letter goes to the next row it begins, round the end.
      if (e.key.length !== 1 || e.ctrlKey || e.metaKey || e.altKey || !/\S/.test(e.key)) return false;
      const letter = e.key.toLowerCase();
      for (let k = 1; k <= rows.length; k++) {
        const row = rows[(at + k + rows.length) % rows.length];
        const label = row.querySelector(".menu-item-label")?.textContent ?? row.textContent ?? "";
        if (label.trim().toLowerCase().startsWith(letter)) {
          row.focus({ preventScroll: true });
          break;
        }
      }
    }
  }
  e.preventDefault();
  e.stopPropagation();
  return true;
}
