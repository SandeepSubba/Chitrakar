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
      forget();
      setTimeout(() => {
        if (chip.isConnected) return;
        if (!row?.isConnected) return;
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
