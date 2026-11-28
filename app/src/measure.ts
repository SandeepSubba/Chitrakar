/** The distances between two layers, as the Move tool shows them with
 * alt held: what is picked against what the pointer is over, or against
 * the frame or page it sits in when the pointer is over nothing.
 *
 * Pure arithmetic over boxes in document units, so it is tested without
 * a browser (`e2e/measure.test.mjs`) and the canvas only draws what it
 * says. The way a designer reads a layout: side by side on an axis,
 * the gap between them on it; one wholly inside the other, how far in it
 * sits from each of the four sides; overlapping without either holding
 * the other, nothing on that axis, since no one distance says where
 * they are. Inside on one axis alone is not inside — a narrower box
 * below another says how far below, not how far in from its sides. */

/** `[x0, y0, x1, y1]`, the left, top, right and bottom edges. */
export type Box = [number, number, number, number];

/** One distance: a line from `from` to `to`, level or upright, and how
 * long it is. */
export interface Gap {
  from: [number, number];
  to: [number, number];
  length: number;
}

/** Shorter than this is touching, and says nothing worth a label. */
const TOUCHING = 0.005;

/** A box out of `bounds_of`'s `[x, y, w, h]`, or nothing for no box. */
export function boxOf(b: ArrayLike<number>): Box | null {
  if (b.length !== 4 || !(b[2] >= 0) || !(b[3] >= 0)) return null;
  return [b[0], b[1], b[0] + b[2], b[1] + b[3]];
}

/** The box round several. */
export function union(boxes: Box[]): Box | null {
  if (boxes.length === 0) return null;
  return boxes.reduce((u, b) => [
    Math.min(u[0], b[0]),
    Math.min(u[1], b[1]),
    Math.max(u[2], b[2]),
    Math.max(u[3], b[3]),
  ]);
}

/** Whether `outer` holds `inner` whole. */
function holds(outer: Box, inner: Box): boolean {
  return outer[0] <= inner[0] && outer[1] <= inner[1] && inner[2] <= outer[2] && inner[3] <= outer[3];
}

/** Along one axis, a's edges and b's on it: the spans to label, each as
 * [start, end] on this axis — the gap between them when they are side by
 * side, how far in from each side when one is `nested` in the other. */
function along(
  a0: number,
  a1: number,
  b0: number,
  b1: number,
  nested: boolean,
): [number, number][] {
  if (b0 >= a1) return [[a1, b0]];
  if (a0 >= b1) return [[b1, a0]];
  if (!nested) return [];
  return [
    [Math.min(a0, b0), Math.max(a0, b0)],
    [Math.min(a1, b1), Math.max(a1, b1)],
  ];
}

/** Where on the other axis a line between the two is drawn: through the
 * middle of the stretch they share, or through the middle of the picked
 * one where they share none. */
function middle(a0: number, a1: number, b0: number, b1: number): number {
  const lo = Math.max(a0, b0);
  const hi = Math.min(a1, b1);
  return hi >= lo ? (lo + hi) / 2 : (a0 + a1) / 2;
}

/** The distances from `a` (what is picked) to `b` (what it is measured
 * against), level ones first. */
export function gapsBetween(a: Box, b: Box): Gap[] {
  const out: Gap[] = [];
  const nested = holds(a, b) || holds(b, a);
  const y = middle(a[1], a[3], b[1], b[3]);
  for (const [x0, x1] of along(a[0], a[2], b[0], b[2], nested)) {
    if (x1 - x0 > TOUCHING) out.push({ from: [x0, y], to: [x1, y], length: x1 - x0 });
  }
  const x = middle(a[0], a[2], b[0], b[2]);
  for (const [y0, y1] of along(a[1], a[3], b[1], b[3], nested)) {
    if (y1 - y0 > TOUCHING) out.push({ from: [x, y0], to: [x, y1], length: y1 - y0 });
  }
  return out;
}
