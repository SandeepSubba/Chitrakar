/**
 * A number typed into a box, the way a hand types one: what is typed is
 * kept as typed while the box has the caret, and a value is handed on
 * only once it is a whole number in range. So "-" on the way to "-5",
 * "1" on the way to "12" with a least of 3, and an emptied box on the way
 * to a new number are passed through rather than turned at once into 0,
 * the least or the most — which is what every box here used to do, since
 * `Number("")` is 0 and each keystroke was clamped as it came. When the
 * caret leaves, what is there is clamped into range and handed on, or,
 * if it is not a number at all, the box shows the value it had.
 *
 * Keys stay in the box (a digit is not a tool's key), except Escape,
 * which a window round it still hears; in the panel, where Escape would
 * let go of the layer, it lets go of the box instead, dropping what was
 * typed and not yet a number.
 */
import { useEffect, useRef, useState } from "react";

type Props = {
  value: number;
  onValue: (v: number) => void;
  min?: number;
  max?: number;
  /** Whole numbers only. */
  integer?: boolean;
  /** What an emptied box means when the caret leaves it (a long edge
   * emptied is "none"); without it, an emptied box goes back. */
  blankAs?: number;
  /** Show this rather than the value (an empty box for "none"). */
  show?: string;
} & Omit<React.InputHTMLAttributes<HTMLInputElement>, "value" | "onChange" | "type" | "min" | "max">;

export function NumberField({ value, onValue, min, max, integer, blankAs, show, onKeyDown, onBlur, onFocus, ...rest }: Props) {
  const [draft, setDraft] = useState<string | null>(null);
  /** Escape let go of the box: what was typed is dropped, not settled. */
  const dropping = useRef(false);
  /** The value this box last handed on: a value that comes back as
   * anything else was changed from outside — an Undo, a preset, a
   * double-click on the slider — and the box shows it rather than what
   * was being typed. */
  const handed = useRef(value);
  useEffect(() => {
    if (value !== handed.current) {
      handed.current = value;
      setDraft(null);
    }
  }, [value]);
  const give = (v: number) => {
    handed.current = v;
    onValue(v);
  };
  const shown = show ?? (Number.isFinite(value) ? String(Math.round(value * 1000) / 1000) : "");
  const fits = (v: number) => (min === undefined || v >= min) && (max === undefined || v <= max);
  const read = (text: string): number | null => {
    if (text.trim() === "") return null;
    const v = Number(text);
    if (!Number.isFinite(v)) return null;
    return integer ? Math.round(v) : v;
  };
  const settle = (text: string) => {
    setDraft(null);
    const v = read(text);
    if (v === null) {
      if (text.trim() === "" && blankAs !== undefined && blankAs !== value) give(blankAs);
      return;
    }
    const held = Math.min(max ?? Infinity, Math.max(min ?? -Infinity, v));
    if (held !== value) give(held);
  };
  return (
    <input
      {...rest}
      type="number"
      min={min}
      max={max}
      value={draft ?? shown}
      onFocus={(e) => {
        setDraft(e.currentTarget.value);
        onFocus?.(e);
      }}
      onChange={(e) => {
        const text = e.target.value;
        setDraft(text);
        const v = read(text);
        if (v !== null && fits(v) && v !== value) give(v);
        else if (text.trim() === "" && blankAs !== undefined && blankAs !== value && e.target.validity.valid) give(blankAs);
      }}
      onBlur={(e) => {
        if (dropping.current) {
          dropping.current = false;
          setDraft(null);
        } else {
          settle(e.currentTarget.value);
        }
        onBlur?.(e);
      }}
      onKeyDown={(e) => {
        onKeyDown?.(e);
        if (e.key === "Enter") settle(e.currentTarget.value);
        if (e.key === "Escape") {
          if (e.currentTarget.closest(".modal")) return;
          dropping.current = true;
          e.currentTarget.blur();
        }
        // Its own keys stay its own, but a chord goes on to the editor:
        // undo or save with the caret here did nothing.
        if (!e.ctrlKey && !e.metaKey) e.stopPropagation();
      }}
    />
  );
}
