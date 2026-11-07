/**
 * The watermark's controls, the same in the Export window and in batch
 * export: whether there is one, its words, where it stands, how big
 * against the picture's shorter side, and white or black. The engine lays
 * it on as the picture goes out (`set_output_watermark`); the document
 * never holds it.
 */
import { WATERMARK_CORNERS, type Watermark } from "./prefs";

const CORNERS: Record<Watermark["corner"], string> = {
  "bottom-right": "Bottom right",
  "bottom-left": "Bottom left",
  "top-right": "Top right",
  "top-left": "Top left",
  middle: "Middle",
};

const SIZES: [string, number][] = [
  ["Small", 0.02],
  ["Medium", 0.03],
  ["Large", 0.05],
];

export function WatermarkRow({
  mark,
  onChange,
  disabled,
}: {
  mark: Watermark;
  onChange: (m: Watermark) => void;
  disabled?: boolean;
}) {
  const set = (part: Partial<Watermark>) => onChange({ ...mark, ...part });
  return (
    <div className="row watermark" role="group" aria-label="Watermark">
      <label className="check">
        <input
          type="checkbox"
          checked={mark.on}
          disabled={disabled}
          onChange={(e) => set({ on: e.target.checked })}
          aria-label="Watermark"
        />
        Watermark
      </label>
      <input
        type="text"
        value={mark.text}
        placeholder="© Your name"
        disabled={disabled}
        onChange={(e) => set({ text: e.target.value, on: mark.on || e.target.value.trim() !== "" })}
        onKeyDown={(e) => e.stopPropagation()}
        aria-label="Watermark text"
      />
      {mark.on && (
        <>
          <select
            value={mark.corner}
            disabled={disabled}
            onChange={(e) => set({ corner: e.target.value as Watermark["corner"] })}
            aria-label="Watermark place"
          >
            {WATERMARK_CORNERS.map((c) => (
              <option key={c} value={c}>
                {CORNERS[c]}
              </option>
            ))}
          </select>
          <select
            value={String(SIZES.find(([, v]) => v === mark.size)?.[1] ?? 0.03)}
            disabled={disabled}
            onChange={(e) => set({ size: Number(e.target.value) })}
            aria-label="Watermark size"
          >
            {SIZES.map(([n, v]) => (
              <option key={n} value={String(v)}>
                {n}
              </option>
            ))}
          </select>
          <select
            value={mark.dark ? "dark" : "light"}
            disabled={disabled}
            onChange={(e) => set({ dark: e.target.value === "dark" })}
            aria-label="Watermark colour"
          >
            <option value="light">White</option>
            <option value="dark">Black</option>
          </select>
        </>
      )}
    </div>
  );
}
