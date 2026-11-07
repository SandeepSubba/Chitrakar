/** What a hand does on glass that a mouse has no word for: taps of more
 * than one finger, and modifier keys held on the screen.
 *
 * Kept apart from the editor so it can be asked without a browser
 * (`e2e/touch.test.mjs`): the editor feeds pointer events in and acts on
 * what comes out. */

/** Taps read off the fingers on the canvas. A tap is every finger down
 * and lifted again quickly, none of them having travelled: two fingers
 * tapped is undo and three is redo, as in every drawing app on a tablet,
 * and the count is what `up` hands back when the last finger leaves. */
export class FingerTaps {
  private start = 0;
  private most = 0;
  private spoiled = false;
  private at = new Map<number, [number, number]>();

  /** `slop` is how far a finger may wander and still be tapping, in
   * screen pixels; `quick` is how long, in milliseconds, from the first
   * finger landing to the last lifting. */
  constructor(
    private slop = 12,
    private quick = 350,
  ) {}

  down(id: number, x: number, y: number, t: number) {
    if (this.at.size === 0) {
      this.start = t;
      this.most = 0;
      this.spoiled = false;
    }
    this.at.set(id, [x, y]);
    this.most = Math.max(this.most, this.at.size);
  }

  move(id: number, x: number, y: number) {
    const from = this.at.get(id);
    if (from && Math.hypot(x - from[0], y - from[1]) > this.slop) this.spoiled = true;
  }

  /** A finger lifted: the number of fingers tapped once the last one
   * has, or 0 while some are still down or when it was not a tap. */
  up(id: number, t: number): number {
    if (!this.at.delete(id) || this.at.size > 0) return 0;
    return !this.spoiled && t - this.start <= this.quick ? this.most : 0;
  }

  /** The system took a finger (a pointercancel), or something else
   * claimed the gesture: whatever this was, it is not a tap. */
  spoil(id?: number) {
    if (id !== undefined) this.at.delete(id);
    this.spoiled = true;
  }
}

/** The modifier keys a screen with no keyboard holds down for you. */
export type Held = { shift: boolean; alt: boolean; mod: boolean };

export const NONE_HELD: Held = { shift: false, alt: false, mod: false };

/** Make a pointer or mouse event say the held keys are down, as a real
 * keyboard's would. Every handler in the editor reads `e.shiftKey` and
 * `e.altKey` off the event it was handed, so changing the event once,
 * before any of them sees it, is what makes all of them — the canvas,
 * the handles, the layer list's shift-click — answer to the latch
 * without any of them knowing it exists. A key really held stays held:
 * this only ever adds. `mod` is control, or command on a Mac, the same
 * reading the editor's chords take. */
export function wear(e: Event, held: Held, mac: boolean) {
  const on = (key: string) => Object.defineProperty(e, key, { value: true, configurable: true });
  if (held.shift) on("shiftKey");
  if (held.alt) on("altKey");
  if (held.mod) on(mac ? "metaKey" : "ctrlKey");
}

/** How a press on one of the screen's modifier keys ends. Tapped, it
 * stays down until tapped again — the way to hold shift with the same
 * hand that then draws; held while the other hand works, it lets go
 * with the finger, as a key does. `wasOn` is whether it was latched
 * before this press began. */
export function afterPress(wasOn: boolean, heldFor: number, quick = 350): boolean {
  if (heldFor > quick) return false;
  return !wasOn;
}
