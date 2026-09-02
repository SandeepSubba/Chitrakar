/** Icon set for the chrome: one stroke weight, one 24-unit grid, all drawn
 * in `currentColor` so a button's state (hover, active, disabled) colours
 * its glyph without a second rule. Kept as paths rather than a font or
 * sprite sheet so the bundle stays a single file and nothing loads late. */

export type IconName =
  | "move"
  | "rect"
  | "ellipse"
  | "pen"
  | "text"
  | "undo"
  | "redo"
  | "fit"
  | "zoomIn"
  | "zoomOut"
  | "proof"
  | "gamut"
  | "newDoc"
  | "open"
  | "image"
  | "save"
  | "export"
  | "profile"
  | "check"
  | "raise"
  | "lower"
  | "group"
  | "ungroup"
  | "brush"
  | "duplicate"
  | "shadow"
  | "crop"
  | "copy"
  | "cut"
  | "paste"
  | "selectAll"
  | "actualSize"
  | "union"
  | "subtract"
  | "intersect"
  | "exclude"
  | "trash"
  | "eye"
  | "eyeOff"
  | "mask"
  | "group-layer"
  | "adjust"
  | "filter"
  | "alignLeft"
  | "alignCenterH"
  | "alignRight"
  | "alignTop"
  | "alignMiddleV"
  | "alignBottom"
  | "distributeH"
  | "distributeV";

const PATHS: Record<IconName, JSX.Element> = {
  // Tools
  crop: (
    <>
      <path d="M6.5 2.5v15h15" />
      <path d="M2.5 6.5h15v15" />
    </>
  ),
  move: <path d="M5 3l14 8-6 1.6L10.5 19z" />,
  rect: <rect x="4" y="6" width="16" height="12" rx="1.5" />,
  ellipse: <ellipse cx="12" cy="12" rx="8" ry="6" />,
  pen: (
    <>
      <path d="M5 19l1.2-4.2L15 6a2 2 0 0 1 2.8 2.8L9.2 17.8z" />
      <path d="M4.6 19.4l3.2-1" />
    </>
  ),
  brush: (
    <>
      <path d="M9.5 14.5c-1.6.6-2 2.2-2.2 3.6-.1.9-.8 1.5-1.7 1.6.9 1 2.2 1.4 3.5 1.1 1.7-.4 2.8-1.9 2.7-3.6" />
      <path d="M11 16.5L19.2 8a2 2 0 0 0-2.8-2.8L8 13.4" />
    </>
  ),
  text: (
    <>
      <path d="M5 6h14" />
      <path d="M12 6v13" />
      <path d="M9 19h6" />
    </>
  ),
  // History and view
  undo: (
    <>
      <path d="M4 10h9a5 5 0 0 1 0 10h-4" />
      <path d="M8 6l-4 4 4 4" />
    </>
  ),
  redo: (
    <>
      <path d="M20 10h-9a5 5 0 0 0 0 10h4" />
      <path d="M16 6l4 4-4 4" />
    </>
  ),
  fit: (
    <>
      <path d="M4 9V5h4" />
      <path d="M20 9V5h-4" />
      <path d="M4 15v4h4" />
      <path d="M20 15v4h-4" />
    </>
  ),
  zoomIn: (
    <>
      <circle cx="11" cy="11" r="6" />
      <path d="M15.5 15.5L20 20" />
      <path d="M11 8.5v5M8.5 11h5" />
    </>
  ),
  zoomOut: (
    <>
      <circle cx="11" cy="11" r="6" />
      <path d="M15.5 15.5L20 20" />
      <path d="M8.5 11h5" />
    </>
  ),
  // Colour management
  proof: (
    <>
      <path d="M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12z" />
      <circle cx="12" cy="12" r="2.5" />
    </>
  ),
  gamut: (
    <>
      <path d="M12 4.5l8.5 15h-17z" />
      <path d="M12 10v4" />
      <path d="M12 16.8v.2" />
    </>
  ),
  profile: (
    <>
      <circle cx="12" cy="12" r="7.5" />
      <path d="M12 4.5v15" />
      <path d="M4.5 12h15" />
    </>
  ),
  // Documents
  newDoc: (
    <>
      <path d="M13 4H7a1.5 1.5 0 0 0-1.5 1.5v13A1.5 1.5 0 0 0 7 20h10a1.5 1.5 0 0 0 1.5-1.5V9.5z" />
      <path d="M13 4v5.5h5.5" />
    </>
  ),
  open: <path d="M3.5 18.5V6.5A1 1 0 0 1 4.5 5.5h4l2 2.5h8a1 1 0 0 1 1 1v9.5a1 1 0 0 1-1 1h-14a1 1 0 0 1-1-1z" />,
  image: (
    <>
      <rect x="4" y="5.5" width="16" height="13" rx="1.5" />
      <circle cx="9" cy="10" r="1.5" />
      <path d="M4.5 16.5l4.5-4 3.5 3 3-2.5 4 3.5" />
    </>
  ),
  save: (
    <>
      <path d="M5 5.5h11L19 8.5v10a.5.5 0 0 1-.5.5h-13a.5.5 0 0 1-.5-.5z" />
      <path d="M8.5 5.5v4h6v-4" />
      <rect x="8" y="13" width="8" height="6" />
    </>
  ),
  export: (
    <>
      <path d="M12 4v10" />
      <path d="M8.5 10.5L12 14l3.5-3.5" />
      <path d="M5 16.5v2a1.5 1.5 0 0 0 1.5 1.5h11a1.5 1.5 0 0 0 1.5-1.5v-2" />
    </>
  ),
  check: <path d="M5 12.5l4.5 4.5L19 7.5" />,
  // Layer actions
  raise: (
    <>
      <path d="M12 19V6" />
      <path d="M7 11l5-5 5 5" />
    </>
  ),
  lower: (
    <>
      <path d="M12 5v13" />
      <path d="M7 13l5 5 5-5" />
    </>
  ),
  group: (
    <>
      <rect x="4" y="4" width="11" height="11" rx="1.5" />
      <path d="M9 19h9.5a1.5 1.5 0 0 0 1.5-1.5V8" />
    </>
  ),
  ungroup: (
    <>
      <rect x="3.5" y="3.5" width="9" height="9" rx="1.5" />
      <rect x="11.5" y="11.5" width="9" height="9" rx="1.5" />
    </>
  ),
  // Layer kinds and row state
  eye: (
    <>
      <path d="M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12z" />
      <circle cx="12" cy="12" r="2.5" />
    </>
  ),
  eyeOff: (
    <>
      <path d="M4 4l16 16" />
      <path d="M9.6 6.1A9.6 9.6 0 0 1 12 5.5c6 0 9.5 6.5 9.5 6.5a17 17 0 0 1-3.2 4" />
      <path d="M6.3 8.1A17.6 17.6 0 0 0 2.5 12S6 18.5 12 18.5c1.4 0 2.6-.3 3.7-.8" />
    </>
  ),
  mask: (
    <>
      <circle cx="12" cy="12" r="7.5" />
      <path d="M12 4.5a7.5 7.5 0 0 0 0 15z" fill="currentColor" stroke="none" />
    </>
  ),
  "group-layer": <path d="M3.5 18.5V6.5A1 1 0 0 1 4.5 5.5h4l2 2.5h8a1 1 0 0 1 1 1v9.5a1 1 0 0 1-1 1h-14a1 1 0 0 1-1-1z" />,
  adjust: (
    <>
      <path d="M5 7h14M5 12h14M5 17h14" />
      <circle cx="9" cy="7" r="2" />
      <circle cx="15" cy="12" r="2" />
      <circle cx="8" cy="17" r="2" />
    </>
  ),
  filter: (
    <>
      <path d="M4 5.5h16l-6.2 7.3v5.4l-3.6 2v-7.4z" />
    </>
  ),
  // Booleans: two overlapping shapes, with the part that survives filled.
  union: (
    <>
      <path
        d="M4 8.5A3.5 3.5 0 0 1 7.5 5h5A3.5 3.5 0 0 1 16 8.5v3A3.5 3.5 0 0 1 12.5 15h-5A3.5 3.5 0 0 1 4 11.5z"
        fill="currentColor"
        opacity="0.25"
      />
      <path
        d="M8 12.5A3.5 3.5 0 0 1 11.5 9h5A3.5 3.5 0 0 1 20 12.5v3A3.5 3.5 0 0 1 16.5 19h-5A3.5 3.5 0 0 1 8 15.5z"
        fill="currentColor"
        opacity="0.25"
      />
      <rect x="4" y="5" width="12" height="10" rx="3.5" />
      <rect x="8" y="9" width="12" height="10" rx="3.5" />
    </>
  ),
  subtract: (
    <>
      <path d="M4 8.5A3.5 3.5 0 0 1 7.5 5h5A3.5 3.5 0 0 1 16 8.5v.5H11.5A3.5 3.5 0 0 0 8 12.5V15H7.5A3.5 3.5 0 0 1 4 11.5z" fill="currentColor" opacity="0.35" />
      <rect x="4" y="5" width="12" height="10" rx="3.5" />
      <rect x="8" y="9" width="12" height="10" rx="3.5" strokeDasharray="3 3" />
    </>
  ),
  intersect: (
    <>
      <path d="M8 9h8v6H8z" fill="currentColor" opacity="0.35" />
      <rect x="4" y="5" width="12" height="10" rx="3.5" />
      <rect x="8" y="9" width="12" height="10" rx="3.5" />
    </>
  ),
  exclude: (
    <>
      <path d="M4 8.5A3.5 3.5 0 0 1 7.5 5h5A3.5 3.5 0 0 1 16 8.5v.5H8v6H7.5A3.5 3.5 0 0 1 4 11.5z" fill="currentColor" opacity="0.35" />
      <path d="M16 9h.5A3.5 3.5 0 0 1 20 12.5v3A3.5 3.5 0 0 1 16.5 19h-5A3.5 3.5 0 0 1 8 15.5V15h8z" fill="currentColor" opacity="0.35" />
      <rect x="4" y="5" width="12" height="10" rx="3.5" />
      <rect x="8" y="9" width="12" height="10" rx="3.5" />
    </>
  ),
  // Align and distribute: a rule plus the edges that meet it.
  alignLeft: (
    <>
      <path d="M4 4v16" />
      <rect x="7" y="6.5" width="12" height="4" rx="1" />
      <rect x="7" y="13.5" width="7" height="4" rx="1" />
    </>
  ),
  alignCenterH: (
    <>
      <path d="M12 4v16" />
      <rect x="6" y="6.5" width="12" height="4" rx="1" />
      <rect x="8.5" y="13.5" width="7" height="4" rx="1" />
    </>
  ),
  alignRight: (
    <>
      <path d="M20 4v16" />
      <rect x="5" y="6.5" width="12" height="4" rx="1" />
      <rect x="10" y="13.5" width="7" height="4" rx="1" />
    </>
  ),
  alignTop: (
    <>
      <path d="M4 4h16" />
      <rect x="6.5" y="7" width="4" height="12" rx="1" />
      <rect x="13.5" y="7" width="4" height="7" rx="1" />
    </>
  ),
  alignMiddleV: (
    <>
      <path d="M4 12h16" />
      <rect x="6.5" y="6" width="4" height="12" rx="1" />
      <rect x="13.5" y="8.5" width="4" height="7" rx="1" />
    </>
  ),
  alignBottom: (
    <>
      <path d="M4 20h16" />
      <rect x="6.5" y="5" width="4" height="12" rx="1" />
      <rect x="13.5" y="10" width="4" height="7" rx="1" />
    </>
  ),
  distributeH: (
    <>
      <path d="M4 4v16M20 4v16" />
      <rect x="10" y="7" width="4" height="10" rx="1" />
    </>
  ),
  distributeV: (
    <>
      <path d="M4 4h16M4 20h16" />
      <rect x="7" y="10" width="10" height="4" rx="1" />
    </>
  ),
  // A shape with its own shadow cast behind it, offset the way the
  // default effect is.
  shadow: (
    <>
      <rect x="8" y="8" width="12" height="12" rx="1.5" fill="currentColor" stroke="none" opacity="0.45" />
      <rect x="4" y="4" width="12" height="12" rx="1.5" />
    </>
  ),
  copy: (
    <>
      <rect x="9" y="9" width="11" height="11" rx="1.5" />
      <path d="M15.5 5.5h-10a1 1 0 0 0-1 1v10" />
    </>
  ),
  cut: (
    <>
      <circle cx="6.5" cy="17.5" r="2.5" />
      <circle cx="17.5" cy="17.5" r="2.5" />
      <path d="M8.3 15.7L18 4M15.7 15.7L6 4" />
    </>
  ),
  paste: (
    <>
      <path d="M9 4.5H6.5a1.5 1.5 0 0 0-1.5 1.5v13A1.5 1.5 0 0 0 6.5 20.5h11a1.5 1.5 0 0 0 1.5-1.5V6a1.5 1.5 0 0 0-1.5-1.5H15" />
      <rect x="9" y="3" width="6" height="3.5" rx="1" />
    </>
  ),
  selectAll: (
    <>
      <path d="M4 8V5.5A1.5 1.5 0 0 1 5.5 4H8" />
      <path d="M16 4h2.5A1.5 1.5 0 0 1 20 5.5V8" />
      <path d="M20 16v2.5a1.5 1.5 0 0 1-1.5 1.5H16" />
      <path d="M8 20H5.5A1.5 1.5 0 0 1 4 18.5V16" />
      <path d="M11 4h2M11 20h2M4 11v2M20 11v2" />
    </>
  ),
  actualSize: (
    <>
      <rect x="4" y="6" width="16" height="12" rx="1.5" />
      <path d="M8.5 14.5v-5l-1.5 1" />
      <path d="M12.5 14.5h3M12.5 14.5v-2h3v-3h-3" />
    </>
  ),
  duplicate: (
    <>
      <rect x="8.5" y="8.5" width="11" height="11" rx="1.5" />
      <path d="M15.5 5.5h-11v11" />
    </>
  ),
  trash: (
    <>
      <path d="M4.5 7h15" />
      <path d="M9.5 7V5.5h5V7" />
      <path d="M6.5 7l.8 12.1a1.5 1.5 0 0 0 1.5 1.4h6.4a1.5 1.5 0 0 0 1.5-1.4L17.5 7" />
      <path d="M10.5 11v6M13.5 11v6" />
    </>
  ),
};

/** A single glyph. `size` is the rendered box; the art is on a 24 grid. */
export function Icon({ name, size = 18 }: { name: IconName; size?: number }) {
  return (
    <svg
      className="icon"
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.6}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      {PATHS[name]}
    </svg>
  );
}
