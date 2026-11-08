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
      forget();
    } else if (e.key === "F2" && rename) {
      e.preventDefault();
      e.stopPropagation();
      rename();
    }
  };
}
