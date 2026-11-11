/** Where the keyboard is while a window is up.
 *
 * Every window of the editor — a dialog over its scrim, the phone's
 * drawer of menus, the feather and straighten panels laid over the
 * canvas — is a React component that mounts and unmounts, and none of
 * them said anything about focus: one opened from a menu left the caret
 * on the page's body and one opened by a chord left it on whatever was
 * under it, Tab walked out of it into the page behind, and closing it
 * dropped the caret on the body again. Rather than teach each of a dozen
 * windows the same three things, this watches the page for them:
 *
 * - A window that appears takes the focus, on its first field (or what
 *   it already focused itself, as Find and replace does).
 * - Tab and Shift+Tab go round inside a modal window and not out of it;
 *   it is marked `aria-modal`, so what is behind is not read either.
 * - A window that goes gives the focus back to what had it before —
 *   for a window opened from a menu row, which is gone by then, the
 *   menu's own label in the bar.
 */

/** A modal window: one over a scrim, which keeps the page behind it. */
const MODAL = ".modal-scrim [role=dialog], .sheet-scrim [role=dialog]";
/** A window that leaves the page reachable: it takes the focus when it
 * appears and gives it back when it goes, but Tab may leave it. */
const PANEL = ".straighten-panel";

const FOCUSABLE = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled]):not([type=hidden])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "[tabindex]:not([tabindex='-1'])",
  "[contenteditable=true]",
].join(", ");

/** What can take the focus inside `root`, in Tab order as far as the
 * page's own order goes (nothing here sets a positive tabindex). */
export function focusables(root: Element): HTMLElement[] {
  return [...root.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(
    (el) => el.tabIndex >= 0 && !el.closest("[aria-hidden=true], [inert]") && el.getClientRects().length > 0,
  );
}

/** The field a window should open on: its first that takes typing or a
 * choice, else whatever comes first — a window of buttons opens on its
 * first button, which for every window here is never the one that goes
 * ahead. */
function firstStop(win: Element): HTMLElement | null {
  const all = focusables(win);
  return (
    all.find(
      (el) =>
        el instanceof HTMLSelectElement ||
        el instanceof HTMLTextAreaElement ||
        (el instanceof HTMLInputElement && el.type !== "checkbox" && el.type !== "radio"),
    ) ??
    all[0] ??
    null
  );
}

/** Whether the main pointer is a finger. */
const coarse = () => typeof matchMedia === "function" && matchMedia("(pointer: coarse)").matches;

let watching = false;

export function watchWindowFocus(doc: Document = document): void {
  if (watching || typeof MutationObserver === "undefined") return;
  watching = true;
  // What had the focus last outside every window: where a window gives
  // it back. A menu's row is the menu's label, since the row is gone by
  // the time the window it opened closes.
  let returnTo: HTMLElement | null = null;
  doc.addEventListener(
    "focusin",
    (e) => {
      const el = e.target;
      if (!(el instanceof HTMLElement) || el.closest(`${MODAL}, ${PANEL}`)) return;
      // Focus that got out of a modal window some way Tab's own round
      // does not cover (a group of radios, a click on the scrim's edge)
      // is put back in it.
      const modals = doc.querySelectorAll(MODAL);
      const top = modals[modals.length - 1];
      if (top) {
        firstStop(top)?.focus({ preventScroll: true });
        return;
      }
      const menu = el.closest(".menu");
      returnTo = (menu?.querySelector<HTMLElement>(".menu-label") ?? el) as HTMLElement;
    },
    true,
  );
  const open = new Map<Element, HTMLElement | null>();
  const settle = () => {
    const now = new Set<Element>(doc.querySelectorAll(`${MODAL}, ${PANEL}`));
    for (const [win, back] of open) {
      if (now.has(win)) continue;
      open.delete(win);
      // Given back only if nothing else has taken it meanwhile — another
      // window opening from this one has it, and keeps it.
      const lost = doc.activeElement === null || doc.activeElement === doc.body;
      if (lost && back?.isConnected) back.focus({ preventScroll: true });
    }
    for (const win of now) {
      if (open.has(win)) continue;
      open.set(win, returnTo);
      if (win.matches(MODAL)) win.setAttribute("aria-modal", "true");
      if (win.contains(doc.activeElement)) continue;
      // On a touch screen a field given the focus brings the keyboard up
      // over half the window before anything is asked of it, so there
      // the window itself takes the focus and Tab begins inside it.
      if (coarse() && win instanceof HTMLElement) {
        if (!win.hasAttribute("tabindex")) win.setAttribute("tabindex", "-1");
        win.focus({ preventScroll: true });
      } else {
        firstStop(win)?.focus({ preventScroll: true });
      }
    }
  };
  new MutationObserver(settle).observe(doc.body, { childList: true, subtree: true });
  settle();
  // Round and round inside the window on top.
  doc.addEventListener(
    "keydown",
    (e) => {
      if (e.key !== "Tab" || e.defaultPrevented) return;
      const modals = doc.querySelectorAll(MODAL);
      const top = modals[modals.length - 1];
      if (!top) return;
      const stops = focusables(top);
      if (stops.length === 0) {
        e.preventDefault();
        return;
      }
      const at = stops.indexOf(doc.activeElement as HTMLElement);
      const first = stops[0];
      const last = stops[stops.length - 1];
      if (at === -1) {
        e.preventDefault();
        (e.shiftKey ? last : first).focus();
      } else if (!e.shiftKey && at === stops.length - 1) {
        e.preventDefault();
        first.focus();
      } else if (e.shiftKey && at === 0) {
        e.preventDefault();
        last.focus();
      }
    },
    true,
  );
}

/** The layers panel, whose rows and buttons go away under the focus. */
const KEPT_IN = "aside.panel";

let keeping = false;

/** Keeps the keyboard's place in the layers panel when what held it
 * goes: a deleted row, a removed effect or gradient stop, a button that
 * the deletion disabled (Delete selected layer, Ungroup). The focus fell
 * to the page, and the next Tab began again from the top of it. It goes
 * to the nearest part of the panel still there — the picked row if that
 * part holds one, else its first stop. Only while the focus was the
 * panel's when it went (a row's menu counts, as the menu gives it back
 * to the row): a press elsewhere lets go, so a layer deleted from the
 * canvas does not pull the focus over. */
export function keepPanelFocus(doc: Document = document): void {
  if (keeping || typeof MutationObserver === "undefined") return;
  keeping = true;
  let held: HTMLElement | null = null;
  let chain: Element[] = [];
  const inMenu = (el: Element) => !!el.closest("[role=menu]");
  doc.addEventListener(
    "focusin",
    (e) => {
      const el = e.target;
      if (!(el instanceof HTMLElement)) return;
      const panel = el.closest(KEPT_IN);
      // Only a button or a row is kept after: a field the panel took away
      // as it showed another layer was being typed into, not pressed, and
      // a button the focus was handed to then took Space from the hand.
      const field =
        el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement || el instanceof HTMLSelectElement || el.isContentEditable;
      if (panel && field) {
        held = null;
      } else if (panel) {
        held = el;
        chain = [];
        for (let a = el.parentElement; a && a !== panel.parentElement; a = a.parentElement) chain.push(a);
      } else if (!inMenu(el)) {
        held = null;
      }
    },
    true,
  );
  doc.addEventListener(
    "pointerdown",
    (e) => {
      const el = e.target;
      if (el instanceof Element && (el.closest(KEPT_IN) || inMenu(el))) return;
      held = null;
    },
    true,
  );
  let pending = false;
  const check = () => {
    pending = false;
    const lost = held;
    if (!lost) return;
    const gone = !lost.isConnected || (lost as HTMLButtonElement).disabled === true;
    if (!gone) return;
    // Still on the disabled button (the browser lets go of it only when
    // it next draws), or already fallen to the page.
    const active = doc.activeElement;
    if (active && active !== doc.body && active !== lost) return;
    for (const part of chain) {
      if (!part.isConnected) continue;
      // A button or a row, never a field: put in X, the next Delete was
      // typed there rather than taking the layer away.
      const stops = focusables(part).filter(
        (el) => !(el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement || el instanceof HTMLSelectElement) && !el.isContentEditable,
      );
      if (stops.length === 0) continue;
      const picked = stops.find((s) => s.closest(".selected, [aria-selected=true]"));
      (picked ?? stops[0]).focus({ preventScroll: true });
      return;
    }
  };
  const later = () => {
    if (pending) return;
    pending = true;
    requestAnimationFrame(check);
  };
  new MutationObserver(later).observe(doc.body, {
    childList: true,
    subtree: true,
    attributes: true,
    attributeFilter: ["disabled"],
  });
}
