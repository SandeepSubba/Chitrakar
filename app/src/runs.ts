/** Style runs: the algebra of styling part of a text block.
 *
 * The engine holds a block's runs as byte ranges into its text, sorted,
 * non-overlapping, and saying only what they change. A textarea hands
 * out UTF-16 offsets instead, so everything here converts on the way in
 * and rebuilds the whole list on the way out — a block's text is short
 * enough that rebuilding is simpler and safer than patching ranges in
 * place, and it keeps the list in the shape the engine expects. */
import type { AuthoredColor, StyleRun } from "./engine";

/** What one run can say. `null` clears a choice back to the block's. */
export type Styling = {
  fill?: AuthoredColor | null;
  bold?: boolean | null;
  italic?: boolean | null;
  underline?: boolean | null;
  strike?: boolean | null;
  font?: string | null;
};

const KEYS = ["fill", "bold", "italic", "underline", "strike", "font"] as const;

/** The byte offset of a UTF-16 index into `text`: the engine counts
 * bytes and a textarea counts code units, and they part company at the
 * first character that is not ASCII. */
export function byteAt(text: string, index: number): number {
  return new TextEncoder().encode(text.slice(0, index)).length;
}

function says(run: Styling): boolean {
  return KEYS.some((k) => run[k] !== undefined && run[k] !== null);
}

function same(a: Styling, b: Styling): boolean {
  return KEYS.every((k) => JSON.stringify(a[k] ?? null) === JSON.stringify(b[k] ?? null));
}

/** A run reduced to what it actually changes: choices it does not make,
 * and choices that are the block's own anyway, are dropped.
 *
 * The second half matters more than it looks. A run saying "not bold"
 * over a block that is not bold reads as saying nothing — but it is not
 * nothing to the engine, which lets a run override the block. Left
 * behind, it would quietly swallow a later Bold pressed on the block. */
function styling(run: Styling, block: Styling): Styling {
  const out: Styling = {};
  for (const k of KEYS) {
    const v = run[k];
    if (v === undefined || v === null) continue;
    if (JSON.stringify(v) === JSON.stringify(block[k] ?? null)) continue;
    (out as Record<string, unknown>)[k] = v;
  }
  return out;
}

/** Every point in the text where the styling can change. */
function marks(runs: StyleRun[], len: number, extra: number[]): number[] {
  const points = new Set<number>([0, len, ...extra]);
  for (const r of runs) {
    points.add(r.start);
    points.add(r.end);
  }
  return [...points].filter((p) => p >= 0 && p <= len).sort((a, b) => a - b);
}

/** The run governing `start..end`, if one covers the whole of it. */
function governing(runs: StyleRun[], start: number, end: number): StyleRun | undefined {
  return runs.find((r) => r.start <= start && r.end >= end);
}

/** The block's runs with `change` laid over the bytes `start..end`.
 *
 * The list is rebuilt from the boundaries out, so runs that overlapped
 * the range are split, ones that end up saying nothing are dropped, and
 * neighbours saying the same thing are joined back together — which
 * keeps styling a word and then unstyling it from leaving debris.
 *
 * `block` is the block's own styling, so a run that would only repeat it
 * is dropped rather than kept as a run that says nothing. */
export function styleRange(
  text: string,
  runs: StyleRun[],
  start: number,
  end: number,
  change: Styling,
  block: Styling = {},
): StyleRun[] {
  const len = new TextEncoder().encode(text).length;
  const cuts = marks(runs, len, [start, end]);
  const out: StyleRun[] = [];
  for (let i = 0; i + 1 < cuts.length; i++) {
    const [a, b] = [cuts[i], cuts[i + 1]];
    if (a >= b) continue;
    let piece: Styling = styling(governing(runs, a, b) ?? {}, block);
    if (a >= start && b <= end) piece = styling({ ...piece, ...change }, block);
    if (!says(piece)) continue;
    const last = out[out.length - 1];
    if (last && last.end === a && same(last, piece)) last.end = b;
    else out.push({ ...piece, start: a, end: b });
  }
  return out;
}

/** What the whole of `start..end` says for one property, or `undefined`
 * when it is not all of one mind — which is what a toggle needs to know
 * to decide whether pressing it turns the property on or off. */
export function rangeSays<K extends keyof Styling>(
  text: string,
  runs: StyleRun[],
  start: number,
  end: number,
  key: K,
  block: NonNullable<Styling[K]>,
): Styling[K] | undefined {
  const len = new TextEncoder().encode(text).length;
  if (start >= end) return undefined;
  const cuts = marks(runs, len, [start, end]).filter((p) => p >= start && p <= end);
  let answer: Styling[K] | undefined;
  for (let i = 0; i + 1 < cuts.length; i++) {
    const [a, b] = [cuts[i], cuts[i + 1]];
    if (a >= b) continue;
    const run = governing(runs, a, b);
    const here = (run?.[key] ?? block) as Styling[K];
    if (i > 0 && JSON.stringify(here) !== JSON.stringify(answer)) return undefined;
    answer = here;
  }
  return answer;
}

/** The runs carried across an edit to the block's text.
 *
 * An edit is read as one replaced stretch — everything between the
 * common prefix and the common suffix — which is what a keystroke, a
 * paste or a deletion actually is. Offsets before it stand, offsets
 * after it move by the change in length, and offsets inside it collapse
 * to where it began. Without this, typing a word in front of a bold one
 * would leave the bold on whatever letters happened to land there. */
export function shiftRuns(before: string, after: string, runs: StyleRun[]): StyleRun[] {
  if (before === after || runs.length === 0) return runs;
  const enc = new TextEncoder();
  const [b, a] = [enc.encode(before), enc.encode(after)];
  // A byte that is not a UTF-8 continuation byte starts a character.
  const starts = (u: Uint8Array, i: number) => i >= u.length || (u[i] & 0xc0) !== 0x80;
  let head = 0;
  while (head < b.length && head < a.length && b[head] === a[head]) head++;
  while (head > 0 && !(starts(b, head) && starts(a, head))) head--;
  let tail = 0;
  while (
    tail < b.length - head &&
    tail < a.length - head &&
    b[b.length - 1 - tail] === a[a.length - 1 - tail]
  )
    tail++;
  while (tail > 0 && !(starts(b, b.length - tail) && starts(a, a.length - tail))) tail--;

  const cutEnd = b.length - tail;
  const delta = a.length - b.length;
  const move = (i: number) => (i <= head ? i : i >= cutEnd ? i + delta : head);
  return runs
    .map((r) => ({ ...r, start: move(r.start), end: move(r.end) }))
    .filter((r) => r.end > r.start);
}
