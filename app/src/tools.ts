/** The tools, and how the rail is laid out from them.
 *
 * These lived at the top of App.tsx as five tables that had to agree
 * with each other by hand. They are one module now because three things
 * read them: the rail, the keyboard handler, and the preferences window
 * — which lets a person put away the tools they never reach for, and
 * has to know what there is to put away. A tool the rail does not show
 * is not gone: its key still works, and it sits in a slot at the end of
 * the rail with the others that were put away, the way Photoshop keeps
 * hidden tools behind its own "…". Nothing is lost; the rail is just
 * shorter.
 */

import type { IconName } from "./icons";

export const TOOLS = [
  "Move",
  "Select",
  "Select ellipse",
  "Lasso",
  "Wand",
  "Frame",
  "Rect",
  "Ellipse",
  "Line",
  "Polygon",
  "Star",
  "Pen",
  "Brush",
  "Paint",
  "Clone",
  "Gradient",
  "Text",
  "Crop",
  "Eyedropper",
  "Hand",
  "Zoom",
] as const;

export type Tool = (typeof TOOLS)[number];

/** The tools that draw a shape, which share one slot in the rail: the
 * one last used sits in it and the rest are a press away, the way a
 * rail with more tools than room has always done it. */
export const SHAPE_TOOLS = ["Rect", "Ellipse", "Line", "Polygon", "Star"] as const;
/** The tools that pick a region out of the page rather than draw
 * anything, sharing one slot the way the shapes do. What they make is a
 * selection: not a layer, not artwork — a region to hand to a layer as
 * the part of it that shows. */
export const SELECT_TOOLS = ["Select", "Select ellipse", "Lasso", "Wand"] as const;

/** The rail, top to bottom, as sections with a line between them. The
 * order is the one every editor shares, near enough: what picks and
 * moves, then what makes things, then what paints on them, then what
 * changes the picture, then what only looks at it. `Select` stands for
 * the region tools' shared slot and `Rect` for the shapes', which is
 * where the slot sits rather than which tool is in it. */
export const RAIL: readonly (readonly Tool[])[] = [
  ["Move", "Select"],
  ["Frame", "Rect", "Pen", "Text"],
  ["Brush", "Paint", "Clone", "Gradient"],
  ["Crop", "Eyedropper"],
  ["Hand", "Zoom"],
];

/** The one tool that cannot be put away: with nothing to move things by,
 * there is no editor. */
export const ALWAYS_SHOWN: Tool = "Move";

/** One letter per tool, the convention every editor shares. `v` for Move
 * because that is where the muscle memory is. */
export const TOOL_KEYS: Record<string, Tool> = {
  v: "Move",
  // `m` for the marquee, which is where that muscle memory is; Move
  // keeps `v`, which is where its own is.
  m: "Select",
  f: "Frame",
  r: "Rect",
  e: "Ellipse",
  l: "Line",
  y: "Polygon",
  k: "Star",
  p: "Pen",
  b: "Brush",
  n: "Paint",
  s: "Clone",
  g: "Gradient",
  t: "Text",
  c: "Crop",
  i: "Eyedropper",
  h: "Hand",
  z: "Zoom",
};

export const TOOL_HINT: Record<Tool, string> = {
  Move: "V",
  Select: "M",
  "Select ellipse": "M",
  Lasso: "M",
  Wand: "M",
  Frame: "F",
  Rect: "R",
  Ellipse: "E",
  Line: "L",
  Polygon: "Y",
  Star: "K",
  Pen: "P",
  Brush: "B",
  Paint: "N",
  Clone: "S",
  Gradient: "G",
  Text: "T",
  Crop: "C",
  Eyedropper: "I",
  Hand: "H",
  Zoom: "Z",
};

export const TOOL_ICONS: Record<Tool, IconName> = {
  Move: "move",
  Select: "marquee",
  "Select ellipse": "marqueeEllipse",
  Lasso: "lasso",
  Wand: "wand",
  Frame: "frame",
  Rect: "rect",
  Ellipse: "ellipse",
  Line: "line",
  Polygon: "polygon",
  Star: "star",
  Pen: "pen",
  Brush: "brush",
  Paint: "paint",
  Clone: "clone",
  Gradient: "gradient",
  Text: "text",
  Crop: "crop",
  Eyedropper: "eyedropper",
  Hand: "hand",
  Zoom: "zoom",
};

/** A word on what each does, for the window that offers to put it away:
 * a name on its own is enough for a tool in hand, and not for one being
 * decided about. */
export const TOOL_ABOUT: Record<Tool, string> = {
  Move: "pick, move, resize and turn layers",
  Select: "pick a rectangle out of the page",
  "Select ellipse": "pick an ellipse out of the page",
  Lasso: "pick a drawn outline out of the page",
  Wand: "pick out what is one colour",
  Frame: "an artboard: a page within the page",
  Rect: "a rectangle, square-cornered or round",
  Ellipse: "an ellipse or a circle",
  Line: "a straight line",
  Polygon: "a regular polygon",
  Star: "a star",
  Pen: "a path, straight or smooth, one point at a time",
  Brush: "a freehand stroke that lands as a path",
  Paint: "a brush that lays pixels",
  Clone: "paint with what is already there, or heal with it",
  Gradient: "drag a gradient across a shape; alt for a radial one",
  Text: "a block of live text",
  Crop: "cut the page down",
  Eyedropper: "take the colour under the cursor",
  Hand: "drag the view about",
  Zoom: "click to look nearer, alt-click to step back",
};

export const isTool = (name: unknown): name is Tool =>
  typeof name === "string" && (TOOLS as readonly string[]).includes(name);
