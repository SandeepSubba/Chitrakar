/** Finding words across the document's text layers and putting others
 * in their place: Edit › Find and replace.
 *
 * Pure string work over one block at a time, so it is tested without a
 * browser (`e2e/findReplace.test.mjs`). What makes it more than a string
 * replace is that a block's styling is kept as byte ranges over its text
 * (`StyleRun`), and every replacement moves the ranges after it; each is
 * carried through `shiftRuns`, one replacement at a time, which is the
 * arithmetic a keystroke in the block already goes through — so a bold
 * word replaced is still bold, and the bold word after it is still that
 * word. */

import type { StyleRun } from "./engine";
import { shiftRuns } from "./runs";

export interface FindOptions {
  /** `Cat` is not `cat`. */
  matchCase: boolean;
  /** `cat` is not the start of `category`: a match is not next to a
   * letter, a digit or an underscore on either side. */
  wholeWord: boolean;
}

/** What is looked for, as a pattern that finds it: the words literally,
 * every character that means something to a pattern escaped. Nothing
 * for nothing to find. */
function pattern(find: string, opts: FindOptions): RegExp | null {
  if (find.length === 0) return null;
  const literal = find.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const word = "[\\p{L}\\p{N}_]";
  const body = opts.wholeWord ? `(?<!${word})${literal}(?!${word})` : literal;
  return new RegExp(body, opts.matchCase ? "gu" : "giu");
}

/** Where `find` is in `text`, left to right and never overlapping, as
 * [start, end) in the string's own (UTF-16) offsets. */
export function matchesIn(text: string, find: string, opts: FindOptions): [number, number][] {
  const re = pattern(find, opts);
  if (!re) return [];
  const out: [number, number][] = [];
  for (const m of text.matchAll(re)) {
    const at = m.index ?? 0;
    out.push([at, at + m[0].length]);
  }
  return out;
}

/** Every match of `find` in a block replaced by `replace`, the block's
 * runs carried across each replacement in turn. `count` is how many
 * were replaced; with none, the block comes back as it was. */
export function replaceIn(
  text: string,
  runs: StyleRun[],
  find: string,
  replace: string,
  opts: FindOptions,
): { text: string; runs: StyleRun[]; count: number } {
  const found = matchesIn(text, find, opts);
  let [out, carried, shift] = [text, runs, 0];
  for (const [start, end] of found) {
    const [from, to] = [start + shift, end + shift];
    const next = out.slice(0, from) + replace + out.slice(to);
    carried = shiftRuns(out, next, carried);
    out = next;
    shift += replace.length - (end - start);
  }
  return { text: out, runs: carried, count: found.length };
}
