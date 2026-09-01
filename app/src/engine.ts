/**
 * TypeScript mirror of the engine's command protocol (serde-JSON encoding of
 * `chitrakar_engine::Command`). The UI never mutates document state itself:
 * it builds commands, sends them over this boundary, and re-renders from
 * engine output.
 *
 * The transport lands with the Phase 0 WASM spike; until then `sendCommand`
 * only logs, so the UI shell can be built and exercised.
 */

export type NodeId = number;

export type BlendMode = "Normal" | "Multiply" | "Screen";

export type AuthoredColor =
  | { Srgb: { r: number; g: number; b: number; a: number } }
  | { Cmyk: { c: number; m: number; y: number; k: number; a: number } };

export type Command =
  | { AddNode: { parent: NodeId; index: number; node: unknown } }
  | { RemoveNode: { id: NodeId } }
  | { SetOpacity: { id: NodeId; opacity: number } }
  | { SetVisible: { id: NodeId; visible: boolean } }
  | { SetBlendMode: { id: NodeId; blend: BlendMode } };

export function sendCommand(cmd: Command): void {
  // WASM engine binding goes here (wasm-bindgen import).
  console.debug("engine command", JSON.stringify(cmd));
}
