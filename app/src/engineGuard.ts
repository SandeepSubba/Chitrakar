/** What happens when the engine stops for good.
 *
 * The engine is Rust compiled to WebAssembly, and a fault in it — a panic,
 * memory it could not have, a stack run out — is a trap that WebAssembly
 * cannot recover from: the module's state is left half-written, and every
 * later call on the session refuses ("recursive use of an object", or the
 * same trap again). The page used to carry on calling it, React gave up,
 * and the editor went blank with the work in it. The engine refuses what
 * it can see coming (a picture too large to hold, a file nested too
 * deep); this is for what it could not.
 *
 * A session handed through `guardSession` notices the first trap, says
 * so once, and opens the page again — where the crash-recovery draft,
 * written a breath after every edit, offers the work back as it was
 * before the step that stopped it.
 */

/** Whether an error is the engine having stopped, rather than refusing. */
export function isEngineTrap(err: unknown): boolean {
  const said = String((err as { message?: unknown })?.message ?? err);
  return (
    err instanceof WebAssembly.RuntimeError ||
    /\bunreachable\b|memory access out of bounds|recursive use of an object|null pointer passed to rust|Maximum call stack size exceeded/i.test(
      said,
    )
  );
}

let stopped = false;

/** Say once that the engine stopped, and open the page again. */
export function engineStopped(err: unknown): void {
  if (stopped) return;
  stopped = true;
  console.error("engine stopped:", err);
  try {
    alert(
      "The editor's engine stopped on the last thing it was asked to do, and cannot go on. " +
        "The page will open again and offer your work as it was just before.",
    );
  } finally {
    location.reload();
  }
}

/** A session whose every call is watched for the engine stopping. */
export function guardSession<T extends object>(session: T): T {
  return new Proxy(session, {
    get(target, prop) {
      const value = Reflect.get(target, prop, target);
      if (typeof value !== "function") return value;
      return (...args: unknown[]) => {
        try {
          return value.apply(target, args);
        } catch (err) {
          if (isEngineTrap(err)) engineStopped(err);
          throw err;
        }
      };
    },
  });
}

/** And any trap that reaches the window some other way — a frame drawn
 * from a timer, a worker's answer — is the same news. */
export function watchForEngineTraps(): void {
  // Not a stack run out here: the page's own script can do that too, and
  // only a call into the engine is known to have been the engine's.
  const trapped = (err: unknown) =>
    isEngineTrap(err) && !/Maximum call stack size exceeded/i.test(String((err as Error)?.message ?? err));
  window.addEventListener("error", (e) => {
    if (trapped(e.error ?? e.message)) engineStopped(e.error ?? e.message);
  });
  window.addEventListener("unhandledrejection", (e) => {
    if (trapped(e.reason)) engineStopped(e.reason);
  });
}
