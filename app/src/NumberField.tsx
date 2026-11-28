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
 * Keys stay in the box (a digit is not a tool's key), except Escape and
 * Enter, which a window round it still hears; in the panel, where Escape would
 * let go of the layer, it lets go of the box instead, dropping what was
 * typed and not yet a number.
 */
import { useEffect, useRef, useState } from "react";
import { flushSync } from "react-dom";
import { commaIsDecimal, plainDigits, readWritten, settleTyped } from "./numberText";

// In every number box on the page: Arabic-Indic or Persian digits are
// digits, and a comma is a decimal or a thousands mark as `numberText`
// reads it. The page is English to the browser, so "12,5" typed into W
// came out 125 and ١٠٠ was dropped without a word; then, read always as
// a decimal, "1,920" typed into New document's width made a page two
// pixels wide. A box of numbers cannot hold a comma, so a typed one goes
// in as a point and is counted, and what it was is settled when the
// caret leaves the box or Enter is pressed: before three digits, or one
// of several, it was a thousands mark.
const typed = new WeakMap<HTMLInputElement, { commas: number; point: boolean }>();
/** Boxes whose whole text was just picked (Ctrl+A): what is typed next
 * replaces it, and what was counted of it goes. */
const wholly = new WeakSet<HTMLInputElement>();
const numberBox = (at: EventTarget | null): at is HTMLInputElement =>
  at instanceof HTMLInputElement && at.type === "number";
if (typeof document !== "undefined") {
  const commaDecimal = commaIsDecimal(typeof navigator !== "undefined" ? navigator.language : undefined);
  document.addEventListener(
    "beforeinput",
    (e) => {
      const at = e.target;
      if (!numberBox(at)) return;
      const data = e.data ?? e.dataTransfer?.getData("text/plain") ?? null;
      if (!data) return;
      let plain: string;
      if (data.length > 1) {
        plain = readWritten(data, commaDecimal);
      } else {
        // What was counted is forgotten once the box is emptied or all
        // of it picked to be typed over: counted on, "2,5" corrected to
        // "7,5" settled as 75.
        const held = at.value;
        const emptied = held === "" && !at.validity.badInput;
        let marks = typed.get(at) ?? { commas: 0, point: false };
        if (emptied || wholly.has(at)) marks = { commas: 0, point: false };
        wholly.delete(at);
        const one = data === "," ? "," : plainDigits(data);
        // The point a comma went in as, taken back out: the comma was a
        // thousands mark after all.
        const unpoint = () => {
          if (!held.includes(".")) return;
          Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set?.call(at, held.replace(/\./g, ""));
        };
        if (one === "," && !commaDecimal && (marks.commas > 0 || marks.point)) {
          // A second comma: the first was a thousands mark, and so is this.
          if (!marks.point) unpoint();
          marks = { commas: marks.commas + 1, point: true };
          plain = "";
        } else if (one === ",") {
          marks = { commas: marks.commas + 1, point: marks.point };
          plain = ".";
        } else if (one === "." && marks.commas > 0 && !marks.point && !commaDecimal) {
          // "1,920.5": a point after a comma makes the comma thousands.
          unpoint();
          marks = { commas: 0, point: true };
          plain = ".";
        } else {
          if (one === ".") marks.point = true;
          plain = one;
        }
        typed.set(at, marks);
      }
      if (plain !== data) {
        e.preventDefault();
        if (plain) document.execCommand("insertText", false, plain);
      }
    },
    true,
  );
  const settle = (at: HTMLInputElement) => {
    const marks = typed.get(at);
    typed.delete(at);
    if (!marks) return;
    const value = settleTyped(at.value, marks.commas, marks.point, commaDecimal);
    if (value === at.value) return;
    // As though typed, so a box that keeps what was typed hears it.
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set?.call(at, value);
    at.dispatchEvent(new Event("input", { bubbles: true }));
  };
  document.addEventListener("focusin", (e) => numberBox(e.target) && typed.delete(e.target), true);
  document.addEventListener("focusout", (e) => numberBox(e.target) && settle(e.target), true);
  document.addEventListener(
    "keydown",
    (e) => {
      if (!numberBox(e.target)) return;
      if (e.key === "Enter") settle(e.target);
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "a") wholly.add(e.target);
      // Moving the caret lets go of the whole — with Ctrl or ⌘ held too
      // (Ctrl+End, ⌘→) — but a modifier alone, or Ctrl+C after Ctrl+A,
      // does not ("7,5" typed then was 75).
      else if (
        e.key.length > 1 &&
        !["Shift", "Control", "Meta", "Alt"].includes(e.key) &&
        (!(e.ctrlKey || e.metaKey) || /^(Arrow|Home|End|Page)/.test(e.key))
      )
        wholly.delete(e.target);
    },
    true,
  );
  // Emptied by Backspace or Delete, the box starts again.
  document.addEventListener(
    "input",
    (e) => {
      const at = e.target;
      if (!numberBox(at)) return;
      if (at.value === "" && !at.validity.badInput) typed.delete(at);
      // The comma's point taken back out: what was counted of it goes
      // with it ("1," Backspace ",5" was 15).
      else if ((e as InputEvent).inputType?.startsWith("delete") && !at.value.includes(".") && !at.validity.badInput) {
        const marks = typed.get(at);
        if (marks && !marks.point) typed.delete(at);
      }
    },
    true,
  );
}

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
  /** The window's own key handler as it stands after the latest render:
   * Enter settles what was typed first, and the window has to act on the
   * settled number, not the one before it. */
  const latestKeyDown = useRef(onKeyDown);
  latestKeyDown.current = onKeyDown;
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
        const inWindow = !!e.currentTarget.closest(".modal, .straighten-panel");
        if (e.key === "Enter") {
          // What was typed is settled — held to the box's range — and the
          // window drawn with it before the window acts: acting first, it
          // used the last number in range on the way (a PDF placed at 200
          // dpi for 2000 typed, a stroke the old width), and the settled
          // one came after as a stray preview of its own.
          const text = e.currentTarget.value;
          flushSync(() => settle(text));
          latestKeyDown.current?.(e);
          // In a window that answers Enter itself (New document, Export)
          // it goes on to the window, unless the box's own handler took
          // it: held here, no box of New document could finish it.
          if (!e.defaultPrevented && inWindow) return;
        } else {
          onKeyDown?.(e);
        }
        if (e.key === "Escape") {
          // A window round the box hears it, the panels Straighten and
          // Feather stand in as much as a modal one.
          if (inWindow) return;
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
