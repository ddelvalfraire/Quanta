// WASD keyboard → 30 Hz `encode_client_input` datagrams paired with the
// predictor's input buffer for Gambetta-style server reconciliation.
//
// Each sent input is tagged with a strictly-monotonic `seq` starting at 1.
// We push the same `(seq, dirX, dirZ)` tuple into the predictor BEFORE we
// write it to the wire — that way the predictor has applied it by the
// time the server acks it, and the replay on reconcile produces the
// predictor's current "known post-server" state exactly.

import { encode_client_input } from "../wasm-decoder/quanta_wasm_decoder";
import { SelfPredictor } from "./predictor";

// Match server `DEMO_TICK_RATE_HZ = 30` in particle-server.rs.
const TICK_PERIOD_MS = Math.round(1000 / 30);

export function startInputLoop(
  wt: WebTransport,
  predictor: SelfPredictor,
): () => void {
  const held = new Set<string>();
  const onDown = (e: KeyboardEvent) => {
    held.add(e.key.toLowerCase());
  };
  const onUp = (e: KeyboardEvent) => {
    held.delete(e.key.toLowerCase());
  };
  window.addEventListener("keydown", onDown);
  window.addEventListener("keyup", onUp);

  const computeDir = (): { dx: number; dz: number } => {
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
    return { dx, dz };
  };

  // rAF-paced setInput so the predictor always has the latest held
  // direction between send intervals — smoother than polling only at
  // TICK_PERIOD_MS boundaries.
  let rafHandle = 0;
  const pumpPrediction = () => {
    const { dx, dz } = computeDir();
    predictor.setInput(dx, dz);
    rafHandle = requestAnimationFrame(pumpPrediction);
  };
  rafHandle = requestAnimationFrame(pumpPrediction);

  let seq = 0;
  const writer = wt.datagrams.writable.getWriter();

  const interval = window.setInterval(() => {
    const { dx, dz } = computeDir();
    seq = (seq + 1) >>> 0;
    // Record the input in the predictor's replay buffer FIRST so the
    // predictor's displayed state already reflects this input. When the
    // server eventually acks `seq`, the predictor will drop it and
    // replay only inputs with higher seq.
    predictor.recordInput(seq, dx, dz);
    const bytes = encode_client_input(0, seq, dx, dz, 0, TICK_PERIOD_MS);
    writer.write(bytes).catch(() => {
      /* connection closing */
    });
  }, TICK_PERIOD_MS);

  return () => {
    window.removeEventListener("keydown", onDown);
    window.removeEventListener("keyup", onUp);
    window.clearInterval(interval);
    cancelAnimationFrame(rafHandle);
    try {
      writer.releaseLock();
    } catch {
      /* writer may already be in use by close path */
    }
  };
}
