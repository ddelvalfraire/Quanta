import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";

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

import { SelfPredictor, MAX_PENDING_INPUTS } from "../predictor.js";

describe("SelfPredictor pending buffer cap (M-4)", () => {
  beforeEach(() => {
    // Anchor performance.now() so recordInput's timestamp stays stable.
    vi.useFakeTimers();
    vi.setSystemTime(new Date(0));
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("exports a finite positive cap", () => {
    expect(Number.isFinite(MAX_PENDING_INPUTS)).toBe(true);
    expect(MAX_PENDING_INPUTS).toBeGreaterThan(0);
  });

  it("never grows the pending buffer beyond MAX_PENDING_INPUTS", () => {
    const predictor = new SelfPredictor();
    const overshoot = MAX_PENDING_INPUTS + 50;
    for (let i = 1; i <= overshoot; i++) {
      predictor.recordInput(i, 1, 0);
      expect(predictor.pendingCount()).toBeLessThanOrEqual(MAX_PENDING_INPUTS);
    }
    expect(predictor.pendingCount()).toBe(MAX_PENDING_INPUTS);
  });

  it("drops oldest inputs when the cap is exceeded, preserving the most recent seq", () => {
    const predictor = new SelfPredictor();
    const overshoot = MAX_PENDING_INPUTS + 10;
    for (let i = 1; i <= overshoot; i++) {
      predictor.recordInput(i, 1, 0);
    }
    // Most recent input must survive.
    expect(predictor.latestPendingSeq()).toBe(overshoot);
  });
});
