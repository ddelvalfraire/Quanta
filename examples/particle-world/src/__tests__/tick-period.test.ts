/**
 * H-2: TICK_PERIOD_MS precision drift
 *
 * Finding: input.ts declares
 *
 *   const TICK_PERIOD_MS = Math.round(1000 / 30);   // → 33  (integer)
 *
 * while state.ts and predictor.ts both use the exact rational value:
 *
 *   state.ts:    export const TICK_PERIOD_MS = 1000 / TICK_RATE_HZ;  // 33.333...
 *   predictor.ts: const TICK_PERIOD_MS = TICK_DT_SEC * 1000;         // 33.333...
 *
 * The result is that the client input loop fires at ~30.30 Hz (1000/33)
 * instead of exactly 30 Hz, while the predictor's interpolation timeline
 * and the state module's virtual clock both assume 33.333 ms per tick.
 * Over time this divergence accumulates: inputs arrive slightly faster than
 * the physics model expects, producing drift in the reconciliation window.
 *
 * EXPECTED STATUS TODAY:
 *   - "Math.round(1000 / 30) equals 1000 / 30" FAILS
 *     because Math.round(1000/30) = 33, not 33.333...
 *   - "Math.round(1000 / 30) equals state.ts TICK_PERIOD_MS" FAILS
 *     because 33 !== 33.333...
 *   - "state.ts TICK_PERIOD_MS is the exact rational 1000/30" PASSES
 *     (documents the correct value for contrast)
 *
 * NOTE: input.ts does not export TICK_PERIOD_MS — it is a private const.
 * The tests encode the finding via:
 *   (a) a direct arithmetic assertion using the same expression input.ts uses
 *   (b) a cross-module comparison against state.ts's exported constant
 */

import { describe, expect, it, vi } from "vitest";

// ---------------------------------------------------------------------------
// Mock the WASM module so state.ts can be imported without a real binary.
// state.ts imports WASM functions only inside method bodies, but the ES
// module loader still executes the WASM JS wrapper's top-level code on
// import.  The mock replaces the entire module with no-op stubs.
// ---------------------------------------------------------------------------

vi.mock(
  "/Users/daviddelval/code/personal/Quanta/examples/particle-world/wasm-decoder/quanta_wasm_decoder.js",
  () => ({
    default: vi.fn().mockResolvedValue(undefined),
    SchemaHandle: class {},
    create_schema: vi.fn().mockReturnValue({}),
    decode_state: vi.fn().mockReturnValue({}),
    apply_delta: vi.fn().mockReturnValue(new Uint8Array()),
    decode_delta_datagram: vi.fn().mockReturnValue({
      flags: 0,
      entitySlot: 0,
      tick: 0n,
      payload: new Uint8Array(),
    }),
    encode_client_input: vi.fn().mockReturnValue(new Uint8Array()),
  }),
);

// Import the exported constant from state.ts — the canonical value used by
// the virtual clock and interpolation code throughout particle-world.
import { TICK_PERIOD_MS as STATE_TICK_PERIOD_MS } from "../state.js";

// ---------------------------------------------------------------------------
// H-2a: The expression input.ts uses (Math.round(1000/30) = 33) is not equal
//        to the exact 30 Hz period (1000/30 = 33.333...).
// ---------------------------------------------------------------------------

describe("H-2a: input.ts TICK_PERIOD_MS equals the exact 30 Hz period", () => {
  it("Math.round(1000 / 30) equals 1000 / 30", () => {
    // This is the literal expression from input.ts line 14:
    //   const TICK_PERIOD_MS = Math.round(1000 / 30);
    //
    // FAILS TODAY: Math.round(1000/30) === 33, not 33.333...
    // vitest output will show:
    //   Expected: 33.33333333333333
    //   Received: 33
    expect(Math.round(1000 / 30)).toBe(1000 / 30);
  });
});

// ---------------------------------------------------------------------------
// H-2b: input.ts and state.ts must use the same TICK_PERIOD_MS value.
//        The predictor's interpolation window (state.ts) and the input
//        firing cadence (input.ts) must agree on tick duration.
// ---------------------------------------------------------------------------

describe("H-2b: input.ts TICK_PERIOD_MS matches state.ts TICK_PERIOD_MS", () => {
  it("Math.round(1000 / 30) equals the TICK_PERIOD_MS exported by state.ts", () => {
    // state.ts exports: TICK_PERIOD_MS = 1000 / TICK_RATE_HZ = 33.333...
    // input.ts uses:    Math.round(1000 / 30)                = 33
    //
    // FAILS TODAY: 33 !== 33.333...
    // vitest output will show:
    //   Expected: 33.33333333333333
    //   Received: 33
    expect(Math.round(1000 / 30)).toBe(STATE_TICK_PERIOD_MS);
  });

  it("state.ts TICK_PERIOD_MS is the exact rational 1000/30", () => {
    // Sanity/documentation check: state.ts itself uses the correct value.
    // This test PASSES today — it documents the correct value for contrast.
    expect(STATE_TICK_PERIOD_MS).toBe(1000 / 30);
  });
});
