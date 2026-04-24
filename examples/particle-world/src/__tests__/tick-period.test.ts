import { describe, expect, it, vi } from "vitest";

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

import { TICK_PERIOD_MS as INPUT_TICK_PERIOD_MS } from "../input.js";
import { TICK_PERIOD_MS as STATE_TICK_PERIOD_MS } from "../state.js";

describe("TICK_PERIOD_MS precision (H-2)", () => {
  it("input.ts uses the exact 30 Hz period", () => {
    expect(INPUT_TICK_PERIOD_MS).toBe(1000 / 30);
  });

  it("input.ts and state.ts agree on the period", () => {
    expect(INPUT_TICK_PERIOD_MS).toBe(STATE_TICK_PERIOD_MS);
  });

  it("state.ts TICK_PERIOD_MS is the exact rational 1000/30", () => {
    expect(STATE_TICK_PERIOD_MS).toBe(1000 / 30);
  });
});
