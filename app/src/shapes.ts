/** The shape library: what the Shape tool draws. Each is a path in a
 * unit box, scaled to the box dragged out, so every one lands as a path
 * — anchors draggable the moment it is drawn, and every exporter already
 * knowing what it is — the way the polygon and the star do. Canva's
 * users draw arrows and callouts more than anything else; these are
 * the ones a survey of the three editors found in all of them. */

export type ShapePreset = {
  name: string;
  /** Anchors in the unit box, y down. */
  points: [number, number][];
  /** Bezier handles per anchor, [inX, inY, outX, outY], as offsets in
   * the unit box; absent for a shape of straight sides. */
  handles?: [number, number, number, number][];
};

export const SHAPE_PRESETS: readonly ShapePreset[] = [
  {
    name: "Triangle",
    points: [
      [0.5, 0],
      [1, 1],
      [0, 1],
    ],
  },
  {
    name: "Diamond",
    points: [
      [0.5, 0],
      [1, 0.5],
      [0.5, 1],
      [0, 0.5],
    ],
  },
  {
    name: "Arrow",
    points: [
      [0, 0.3],
      [0.6, 0.3],
      [0.6, 0],
      [1, 0.5],
      [0.6, 1],
      [0.6, 0.7],
      [0, 0.7],
    ],
  },
  {
    name: "Chevron",
    points: [
      [0, 0],
      [0.7, 0],
      [1, 0.5],
      [0.7, 1],
      [0, 1],
      [0.3, 0.5],
    ],
  },
  {
    name: "Callout",
    points: [
      [0, 0],
      [1, 0],
      [1, 0.72],
      [0.42, 0.72],
      [0.18, 1],
      [0.22, 0.72],
      [0, 0.72],
    ],
  },
  {
    name: "Heart",
    // Six anchors and their handles: the bottom point, the two lobes'
    // outer sides, their tops, and the notch between them.
    points: [
      [0.5, 1],
      [0, 0.35],
      [0.3, 0],
      [0.5, 0.3],
      [0.7, 0],
      [1, 0.35],
    ],
    handles: [
      [0, 0, 0, 0],
      [0, 0.3, 0, -0.2],
      [-0.15, 0, 0.1, 0],
      [0, -0.2, 0, -0.2],
      [-0.1, 0, 0.15, 0],
      [0, -0.2, 0, 0.3],
    ],
  },
];

/** A preset scaled to a box, as the anchors and handles a path takes. */
export function presetPath(
  preset: ShapePreset,
  w: number,
  h: number,
): { points: [number, number][]; handles: [number, number, number, number][] } {
  return {
    points: preset.points.map(([u, v]) => [u * w, v * h]),
    handles: (preset.handles ?? []).map(([ix, iy, ox, oy]) => [ix * w, iy * h, ox * w, oy * h]),
  };
}
