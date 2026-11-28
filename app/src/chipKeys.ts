/** The keys of a chip that stands for something kept by name — a colour
 * in the palette, a style, a comp, a brush, a kept region, a setup.
 * What a modified click does to one (alt-click forgets it, a
 * double-click renames a comp) had no key, so nothing kept could be let
 * go of from the keyboard: Delete or Backspace on a focused chip
 * forgets it, and F2 renames it where it can be renamed. */
export function chipKeys(forget: () => void, rename?: () => void) {
  return (e: React.KeyboardEvent<HTMLElement>) => {
    if (e.key === "Delete" || e.key === "Backspace") {
      // Not the picked layer: the chip had the focus.
      e.preventDefault();
      e.stopPropagation();
      // The focus goes to the chip beside it, or the row's add button:
      // left on a chip that is gone, it fell to the page's body.
      const chip = e.currentTarget;
      const row = chip.parentElement;
      const at = row ? [...row.querySelectorAll<HTMLElement>("button")].indexOf(chip as HTMLElement) : -1;
      // The rows beside it, for when the last chip takes its row with it
      // (the recent colours): the focus fell to the page, and the next
      // Delete took the picked layer.
      const beside = [row?.previousElementSibling, row?.nextElementSibling];
      forget();
      setTimeout(() => {
        if (chip.isConnected) return;
        const left = row?.isConnected ? row.querySelectorAll("button:not(:disabled)").length : 0;
        if (row?.isConnected && left === 0) {
          // Still there with nothing left to press (kept regions with no
          // "+" while nothing is picked out): the row itself holds the
          // focus, where Delete is the row's and forgets nothing. Handed
          // to the bar's button beside it, the next Delete took the
          // picked layer.
          if (row.tabIndex < 0 && !row.hasAttribute("tabindex")) row.tabIndex = -1;
          row.focus();
          return;
        }
        if (!row?.isConnected || left === 0) {
          // Gone, or left with nothing to press (kept regions with no
          // "+" while nothing is picked out).
          for (const near of beside) {
            // The neighbour may be a button itself (the Select tool's
            // bar sets its buttons straight beside the row).
            const stop = !near?.isConnected
              ? null
              : near.matches("button:not(:disabled)")
                ? (near as HTMLElement)
                : near.querySelector<HTMLElement>("button:not(:disabled)");
            if (stop) {
              stop.focus();
              return;
            }
          }
          return;
        }
        const buttons = [...row.querySelectorAll<HTMLElement>("button:not(:disabled)")];
        (buttons[Math.min(at, buttons.length - 1)] ?? buttons[0])?.focus();
      }, 0);
    } else if (e.key === "F2" && rename) {
      e.preventDefault();
      e.stopPropagation();
      rename();
    }
  };
}

/** A finger's way to forget a chip. Alt-click is a keyboard's, and the
 * latched Alt of a touch screen sits on the bar, under the scrim of the
 * develop and export windows, so a kept preset or setup there could not
 * be let go of by touch at all: held for half a second, the chip asks,
 * and the tap that ends the hold gives nothing. */
// One finger holds one chip at a time, and the hold outlives the render
// it began in (the handlers below are made afresh on each), so its state
// is kept here rather than in their closure.
let timer: ReturnType<typeof setTimeout> | undefined;
let held = false;
let at = [0, 0];
const stop = () => {
  if (timer !== undefined) clearTimeout(timer);
  timer = undefined;
};
export function chipHold(name: string, forget: () => void) {
  return {
    onPointerDown: (e: React.PointerEvent<HTMLElement>) => {
      held = false;
      stop();
      if (e.pointerType === "mouse") return;
      at = [e.clientX, e.clientY];
      timer = setTimeout(() => {
        timer = undefined;
        held = true;
        if (confirm(`Forget "${name}"?`)) forget();
      }, 550);
    },
    onPointerMove: (e: React.PointerEvent<HTMLElement>) => {
      // A finger that slides is scrolling the row, not holding the chip.
      if (timer !== undefined && Math.hypot(e.clientX - at[0], e.clientY - at[1]) > 10) stop();
    },
    onPointerUp: stop,
    onPointerCancel: stop,
    onContextMenu: (e: React.MouseEvent<HTMLElement>) => {
      // The system's own menu for a long press (Android's) is not this.
      if (timer !== undefined || held) e.preventDefault();
    },
    onClickCapture: (e: React.MouseEvent<HTMLElement>) => {
      if (!held) return;
      held = false;
      e.preventDefault();
      e.stopPropagation();
    },
  };
}
