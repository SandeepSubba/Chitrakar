/** Equal spacing, as a dragged layer finds it: Figma's spacing guides.
 *
 * Carried past a neighbour, a layer snaps to leave the same gap that
 * neighbour leaves to the next one along; carried between two, it snaps
 * to the middle, the same gap either side. The gaps that are now equal
 * are handed back to be marked. Pure arithmetic over boxes, tested
 * without a browser (`e2e/spacing.test.mjs`); the Move tool asks it on
 * every pointer sample, after the edges and middles have had their say.
 *
 * Only the boxes beside the dragged one count — those overlapping it
 * across the axis being spaced, a row for spacing along x and a column
 * for spacing along y — since a gap between things in different rows is
 * not a gap anybody sees. */

import type { Box, Gap } from "./measure";

/** What equal spacing asks of the dragged box along one axis: how far
 * to move it, and the gaps to mark once it has. */
export interface Spacing {
  delta: number;
  marks: Gap[];
}

/** One axis, with the boxes flipped onto it: `lo`/`hi` along the axis
 * being spaced, `a`/`b` across it. */
interface Span {
  lo: number;
  hi: number;
  a: number;
  b: number;
}

const along = (box: Box, axis: "x" | "y"): Span =>
  axis === "x"
    ? { lo: box[0], hi: box[2], a: box[1], b: box[3] }
    : { lo: box[1], hi: box[3], a: box[0], b: box[2] };

/** A gap from `lo` to `hi` along the axis, drawn across it at `at`. */
const gap = (axis: "x" | "y", lo: number, hi: number, at: number): Gap =>
  axis === "x"
    ? { from: [lo, at], to: [hi, at], length: hi - lo }
    : { from: [at, lo], to: [at, hi], length: hi - lo };

/** Where a line between two spans is drawn across the axis: through the
 * middle of what they share. */
const across = (p: Span, q: Span) => (Math.max(p.a, q.a) + Math.min(p.b, q.b)) / 2;

/** The nearest equal spacing for `moving` among `others` along `axis`,
 * within `tol` of where it is, or nothing. */
export function equalSpacing(
  moving: Box,
  others: Box[],
  axis: "x" | "y",
  tol: number,
): Spacing | null {
  const m = along(moving, axis);
  // Beside it across the axis, in order along it.
  const row = others
    .map((o) => along(o, axis))
    .filter((o) => o.b > m.a && o.a < m.b)
    .sort((p, q) => p.lo - q.lo);
  const width = m.hi - m.lo;
  // The nearest wholly before it and wholly after it, by a little slack
  // so a layer already in place still counts its neighbours as such.
  const before = row.filter((o) => o.hi <= m.lo + tol).sort((p, q) => q.hi - p.hi);
  const after = row.filter((o) => o.lo >= m.hi - tol).sort((p, q) => p.lo - q.lo);
  const options: Spacing[] = [];
  const left = before[0];
  const right = after[0];
  // Past a neighbour on the near side: the gap it leaves to the one
  // before it.
  if (left) {
    const prior = row.filter((o) => o.hi <= left.lo).sort((p, q) => q.hi - p.hi)[0];
    if (prior) {
      const g = left.lo - prior.hi;
      const lo = left.hi + g;
      options.push({
        delta: lo - m.lo,
        marks: [gap(axis, prior.hi, left.lo, across(prior, left)), gap(axis, left.hi, lo, across(left, m))],
      });
    }
  }
  // And on the far side.
  if (right) {
    const next = row.filter((o) => o.lo >= right.hi).sort((p, q) => p.lo - q.lo)[0];
    if (next) {
      const g = next.lo - right.hi;
      const hi = right.lo - g;
      options.push({
        delta: hi - m.hi,
        marks: [gap(axis, hi, right.lo, across(m, right)), gap(axis, right.hi, next.lo, across(right, next))],
      });
    }
  }
  // Between the two: the same gap either side.
  if (left && right && right.lo - left.hi > width) {
    const g = (right.lo - left.hi - width) / 2;
    const lo = left.hi + g;
    options.push({
      delta: lo - m.lo,
      marks: [gap(axis, left.hi, lo, across(left, m)), gap(axis, lo + width, right.lo, across(m, right))],
    });
  }
  const near = options.filter((o) => Math.abs(o.delta) < tol && o.marks.every((k) => k.length > 0));
  if (near.length === 0) return null;
  return near.reduce((best, o) => (Math.abs(o.delta) < Math.abs(best.delta) ? o : best));
}
