/** The desktop shell's menu bar, built from the same description the
 * in-window bar renders — so the two never drift and a menu is written
 * once. In a browser none of this runs and the in-window bar stays.
 *
 * The items carry no accelerators. Every shortcut the menu names is
 * already handled by the app's own keydown handling, which knows what has
 * focus — a native accelerator would take the key first and fire the
 * canvas action while someone is typing in a text layer.
 */

/** One row of a menu: a separator, or something to pick. */
export type MenuEntry =
  | { kind: "sep" }
  | {
      kind: "item";
      id: string;
      /** The glyph the in-window row draws; a native row has none. */
      icon: string;
      label: string;
      hint?: string;
      run: () => void;
    };

/** One menu on the bar. */
export type MenuSpec = { id: string; label: string; entries: MenuEntry[] };

/** Whether the app is running inside the Tauri shell rather than a
 * browser tab. */
export const isTauri = () =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/** What the menu looks like, as a string: rebuilding the native menu on
 * every render would tear it down under the pointer, so the caller only
 * rebuilds when this changes. Labels and hints carry the state that shows
 * — a tick beside the current units, "Hide guides" against "Show
 * guides" — so they are what the signature is made of. */
export const menuSignature = (menus: MenuSpec[]) =>
  menus
    .map(
      (m) =>
        `${m.label}:${m.entries
          .map((e) => (e.kind === "sep" ? "-" : `${e.id}|${e.label}|${e.hint ?? ""}`))
          .join(",")}`,
    )
    .join(";");

/** Build the menu bar and hand it to the OS. `dispatch` is called with
 * the id of whatever was picked; the caller routes it to a handler that
 * is current, since this menu outlives the render that built it. */
export async function setNativeMenu(
  menus: MenuSpec[],
  dispatch: (id: string) => void,
  appName: string,
): Promise<void> {
  const { Menu, MenuItem, PredefinedMenuItem, Submenu } = await import(
    "@tauri-apps/api/menu"
  );

  const sep = () => PredefinedMenuItem.new({ item: "Separator" });

  // macOS expects the first submenu to be the application's own, and it
  // is where About and Quit live whatever else the app offers.
  const app = await Submenu.new({
    text: appName,
    items: await Promise.all([
      PredefinedMenuItem.new({ item: { About: null } }),
      sep(),
      PredefinedMenuItem.new({ item: "Services" }),
      sep(),
      PredefinedMenuItem.new({ item: "Hide" }),
      PredefinedMenuItem.new({ item: "HideOthers" }),
      PredefinedMenuItem.new({ item: "ShowAll" }),
      sep(),
      PredefinedMenuItem.new({ item: "Quit" }),
    ]),
  });

  const submenus = await Promise.all(
    menus.map(async (m) =>
      Submenu.new({
        text: m.label,
        items: await Promise.all(
          m.entries.map((e) =>
            e.kind === "sep"
              ? sep()
              : MenuItem.new({
                  id: e.id,
                  // The hint is the tick or the note the in-window row
                  // puts on its right; a native row has one column, so it
                  // rides along in the text.
                  text: e.hint && e.hint !== "✓" ? `${e.label} (${e.hint})` : e.hint ? `✓ ${e.label}` : e.label,
                  action: () => dispatch(e.id),
                }),
          ),
        ),
      }),
    ),
  );

  // A Window menu, so the window can be minimised and closed the way
  // every other Mac app allows.
  const windowMenu = await Submenu.new({
    text: "Window",
    items: await Promise.all([
      PredefinedMenuItem.new({ item: "Minimize" }),
      PredefinedMenuItem.new({ item: "Maximize" }),
      sep(),
      PredefinedMenuItem.new({ item: "CloseWindow" }),
    ]),
  });

  const menu = await Menu.new({ items: [app, ...submenus, windowMenu] });
  await menu.setAsAppMenu();
}
