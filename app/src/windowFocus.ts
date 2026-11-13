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
 *   for a window opened from a menu row, what had it before the menus.
 */

/** A modal window: one over a scrim, which keeps the page behind it. */
const MODAL = ".modal-scrim [role=dialog], .sheet-scrim [role=dialog]";
/** A window that leaves the page reachable: it takes the focus when it
 * appears and gives it back when it goes, but Tab may leave it. */
const PANEL = ".straighten-panel";
/** The menus: the bar, its popups and the phone's button for them. */
const MENUS = ".menubar, .menu, [role=menu], .menu-sheet-toggle";

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
    // A window of buttons opens on its first button, as said, and not on
    // a tick box before it ("Don't ask again", ticked by the next Space).
    all.find((el) => el instanceof HTMLButtonElement) ??
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
  // it back.
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
        enter(top);
        return;
      }
      // The menus are a way to a window, not a place to come back to:
      // a window opened from a row goes back to where the keyboard (or
      // the pointer) was before the menus — a layer's row, a field, or
      // nothing in particular, where the editor's keys are. Given back
      // to the menu's label, the arrows walked the bar rather than
      // nudging the layer, and Escape let go of it.
      if (el.closest(MENUS)) return;
      returnTo = el;
    },
    true,
  );
  // Focus let go of to the page itself (a press on the canvas) is
  // somewhere too: nothing in particular.
  doc.addEventListener(
    "focusout",
    (e) => {
      const el = e.target;
      if (e.relatedTarget !== null || !(el instanceof HTMLElement)) return;
      if (el.closest(`${MODAL}, ${PANEL}, ${MENUS}`)) return;
      returnTo = null;
    },
    true,
  );
  // On a touch screen a field given the focus brings the keyboard up
  // over half the window before anything is asked of it, so there the
  // window itself takes the focus and Tab begins inside it — whichever
  // way the focus comes to it: a window opened from the phone's drawer
  // was entered by the put-back above, as the drawer gave the focus to
  // its button, and that went to the first field.
  const enter = (win: Element) => {
    if (coarse() && win instanceof HTMLElement) {
      if (!win.hasAttribute("tabindex")) win.setAttribute("tabindex", "-1");
      win.focus({ preventScroll: true });
    } else {
      firstStop(win)?.focus({ preventScroll: true });
    }
  };
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
      enter(win);
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
  /** Where the held stop was on screen, for finding the one nearest. */
  let where: [number, number] = [0, 0];
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
        const r = el.getBoundingClientRect();
        where = [r.left + r.width / 2, r.top + r.height / 2];
        chain = [];
        for (let a = el.parentElement; a && a !== panel.parentElement; a = a.parentElement) chain.push(a);
        // The panel itself last, whatever the depth: a button sitting
        // straight in it (the align bar's) had the whole panel as its
        // second-nearest part, and the history's "As it began" won.
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
    // Near the lost stop first (what it sat in, and that one's own
    // part), then the layer list — a deleted layer's button or a removed
    // effect's left the focus on the comps' "+" or the proportion lock,
    // where the next Enter or Space did something nobody asked for —
    // and only then the panel's wider parts.
    const list = doc.querySelector<HTMLElement>(`${KEPT_IN} .layer-list`);
    const near = chain.slice(0, 2).filter((part) => !part.matches(KEPT_IN));
    const wider = chain.slice(near.length).filter((part) => !near.includes(part));
    const parts = [...near, ...(list ? [list] : []), ...wider];
    for (const part of parts) {
      if (!part.isConnected) continue;
      // A button or a row, never a field: put in X, the next Delete was
      // typed there rather than taking the layer away. Nor a step of the
      // history or the comps' "+", where the next Enter or Space put
      // the page back to its start or kept a comp nobody asked for.
      // A palette's chips are passed over unless the lost stop was one
      // of that palette's own (a style forgotten goes to the next style).
      // In the layer list, a row and not its eye or lock: Escape and then
      // Enter there hid or locked a layer.
      const ownPalette = chain.find((p) => p.matches(".palette")) ?? null;
      const stops = focusables(part).filter(
        (el) =>
          !(el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement || el instanceof HTMLSelectElement) &&
          !el.isContentEditable &&
          !el.closest(".history") &&
          (!el.closest(".palette") || el.closest(".palette") === ownPalette) &&
          !(part === list && /^(Show|Hide|Lock|Unlock) layer/.test(el.getAttribute("aria-label") ?? "")),
      );
      if (stops.length === 0) {
        // The last layer gone: the list holds the place, empty, rather
        // than handing it on to the panel's wider parts.
        if (part === list && list.isConnected) {
          if (!list.hasAttribute("tabindex")) list.setAttribute("tabindex", "-1");
          list.focus({ preventScroll: true });
          return;
        }
        continue;
      }
      // The picked row if this part holds one, else the stop nearest
      // where the lost one was — the next effect's or stop's, the row
      // that took the deleted one's place — rather than the part's
      // first, which for the panel's properties was a tick box far
      // above the list the focus had been in.
      const picked = stops.find((s) => s.closest(".selected, [aria-selected=true]"));
      const far = (s: HTMLElement) => {
        const r = s.getBoundingClientRect();
        return Math.abs(r.top + r.height / 2 - where[1]) * 4 + Math.abs(r.left + r.width / 2 - where[0]);
      };
      const nearest = stops.reduce((a, b) => (far(b) < far(a) ? b : a));
      (picked ?? nearest).focus({ preventScroll: true });
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
