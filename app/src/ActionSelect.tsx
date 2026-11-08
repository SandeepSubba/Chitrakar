import { useRef, useState } from "react";

type Props = Omit<React.SelectHTMLAttributes<HTMLSelectElement>, "onChange" | "value"> & {
  /** What the choice does, given the option's value. */
  onPick: (value: string) => void;
};

/** A drop-down whose options are things to do — add this adjustment,
 * add that effect — rather than a setting it shows.
 *
 * Chosen from its list with the pointer, an option is done at once and
 * the box goes back to its placeholder. From the keyboard a closed
 * select changes on every arrow and every letter, and doing each one
 * added an Exposure layer per press — and since the box then went back
 * to its placeholder, the arrows only ever reached the first option. So
 * the keys browse, the box showing where they are, and Enter does it;
 * leaving the box lets the browsing go. */
export function ActionSelect({ onPick, onKeyDown, onBlur, onPointerDown, children, ...rest }: Props) {
  const [browsing, setBrowsing] = useState("");
  /** Whether the change about to come was made by a key. */
  const keyed = useRef(false);
  return (
    <select
      {...rest}
      value={browsing}
      onPointerDown={(e) => {
        keyed.current = false;
        onPointerDown?.(e);
      }}
      onKeyDown={(e) => {
        if (e.key === "Enter" && browsing) {
          e.preventDefault();
          e.stopPropagation();
          const chosen = browsing;
          setBrowsing("");
          onPick(chosen);
          return;
        }
        if (
          e.key.startsWith("Arrow") ||
          ["Home", "End", "PageUp", "PageDown"].includes(e.key) ||
          (e.key.length === 1 && e.key !== " " && !e.ctrlKey && !e.metaKey && !e.altKey)
        )
          keyed.current = true;
        onKeyDown?.(e);
      }}
      onChange={(e) => {
        const value = e.target.value;
        if (keyed.current) {
          keyed.current = false;
          setBrowsing(value);
          return;
        }
        setBrowsing("");
        if (value) onPick(value);
      }}
      onBlur={(e) => {
        keyed.current = false;
        setBrowsing("");
        onBlur?.(e);
      }}
    >
      {children}
    </select>
  );
}
