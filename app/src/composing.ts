/**
 * Keys an input method is using are its own.
 *
 * Japanese, Chinese and Korean are typed through a composition: the
 * letters typed become a reading, the reading is turned into the words
 * chosen, and while that is going on Escape takes back the candidate and
 * Enter accepts it. The browser says so on the key (`isComposing`, and
 * Safari's accepting Enter by keyCode 229). The editor's keys knew
 * nothing of it: Escape threw away the whole text block being typed,
 * Enter in Find and replace replaced everything with the unconverted
 * reading, and every window closed on the Escape meant for a candidate.
 *
 * Rather than teach every handler, the keys are stopped here, on the
 * window in its capture phase before anything else hears them, the way
 * `touch.ts` wears modifiers onto pointer events: nothing in the editor
 * sees a key the input method is using. Stopping a key does not stop the
 * browser doing what it does with it, so the composition itself goes on
 * as it would anywhere else.
 */
export function composingKey(e: KeyboardEvent): boolean {
  return e.isComposing || e.keyCode === 229;
}

export function holdKeysWhileComposing(target: Window = window): () => void {
  const hold = (e: KeyboardEvent) => {
    if (composingKey(e)) e.stopImmediatePropagation();
  };
  target.addEventListener("keydown", hold, true);
  target.addEventListener("keyup", hold, true);
  return () => {
    target.removeEventListener("keydown", hold, true);
    target.removeEventListener("keyup", hold, true);
  };
}
