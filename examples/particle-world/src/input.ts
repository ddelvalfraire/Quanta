// WASD keyboard loop → 20 Hz `encode_client_input` datagrams.
//
// The server ignores the `entity_slot` field in the payload (uses the
// slot allocated at RegisterClient); passing 0 here matches the fanout
// integration test's behavior.

import { encode_client_input } from "../wasm-decoder/quanta_wasm_decoder";

const TICK_PERIOD_MS = 50;

export function startInputLoop(wt: WebTransport): () => void {
  const held = new Set<string>();
  const onDown = (e: KeyboardEvent) => {
    held.add(e.key.toLowerCase());
  };
  const onUp = (e: KeyboardEvent) => {
    held.delete(e.key.toLowerCase());
  };
  window.addEventListener("keydown", onDown);
  window.addEventListener("keyup", onUp);

  let seq = 0;
  const writer = wt.datagrams.writable.getWriter();

  const interval = window.setInterval(() => {
    let dx = 0;
    let dz = 0;
    if (held.has("d") || held.has("arrowright")) dx += 1;
    if (held.has("a") || held.has("arrowleft")) dx -= 1;
    if (held.has("s") || held.has("arrowdown")) dz += 1;
    if (held.has("w") || held.has("arrowup")) dz -= 1;

    const mag = Math.hypot(dx, dz);
    if (mag > 0) {
      dx /= mag;
      dz /= mag;
    }
    seq = (seq + 1) >>> 0;
    const bytes = encode_client_input(0, seq, dx, dz, 0, TICK_PERIOD_MS);
    writer.write(bytes).catch(() => {
      /* connection closing */
    });
  }, TICK_PERIOD_MS);

  return () => {
    window.removeEventListener("keydown", onDown);
    window.removeEventListener("keyup", onUp);
    window.clearInterval(interval);
    try {
      writer.releaseLock();
    } catch {
      /* writer may already be in use by close path */
    }
  };
}
