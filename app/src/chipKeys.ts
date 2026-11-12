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
