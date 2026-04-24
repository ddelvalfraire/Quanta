import { beforeEach, describe, expect, it, vi } from "vitest";

const mockDecodeState = vi.fn();
const mockDecodeAuthResponse = vi.fn();

vi.mock(
  "/Users/daviddelval/code/personal/Quanta/packages/delta-decoder/wasm/quanta_wasm_decoder.js",
  () => ({
    default: vi.fn().mockResolvedValue(undefined),
    create_schema: vi.fn().mockReturnValue({}),
    decode_state: mockDecodeState,
    decode_auth_response: mockDecodeAuthResponse,
    apply_delta: vi.fn(),
    encode_state: vi.fn(),
    encode_auth_request: vi.fn(),
    encode_auth_response: vi.fn(),
    encode_baseline_ack: vi.fn(),
  }),
);

import { loadDecoder } from "../index.js";

let decoder: Awaited<ReturnType<typeof loadDecoder>>;

beforeEach(async () => {
  vi.clearAllMocks();
  decoder = await loadDecoder(new Uint8Array(0));
});

describe("decodeState runtime validation (H-1a)", () => {
  it("throws when a field value is not number or boolean", () => {
    mockDecodeState.mockReturnValue({ pos_x: "not a number" });
    const fakeSchema = {} as Parameters<typeof decoder.decodeState>[0];
    expect(() =>
      decoder.decodeState(fakeSchema, new Uint8Array([0])),
    ).toThrow(/invalid type string/);
  });

  it("throws when WASM returns null", () => {
    mockDecodeState.mockReturnValue(null);
    const fakeSchema = {} as Parameters<typeof decoder.decodeState>[0];
    expect(() =>
      decoder.decodeState(fakeSchema, new Uint8Array([0])),
    ).toThrow(/non-object/);
  });

  it("throws when WASM returns an array", () => {
    mockDecodeState.mockReturnValue([1, 2, 3]);
    const fakeSchema = {} as Parameters<typeof decoder.decodeState>[0];
    expect(() =>
      decoder.decodeState(fakeSchema, new Uint8Array([0])),
    ).toThrow(/non-object/);
  });
});

describe("decodeAuthResponse runtime validation (H-1b)", () => {
  it("throws when sessionId is a string", () => {
    mockDecodeAuthResponse.mockReturnValue({
      sessionId: "not a bigint",
      accepted: true,
      reason: "",
    });
    expect(() =>
      decoder.decodeAuthResponse(new Uint8Array([0])),
    ).toThrow(/sessionId must be bigint/);
  });

  it("throws when sessionId is a plain number", () => {
    mockDecodeAuthResponse.mockReturnValue({
      sessionId: 42,
      accepted: true,
      reason: "",
    });
    expect(() =>
      decoder.decodeAuthResponse(new Uint8Array([0])),
    ).toThrow(/sessionId must be bigint/);
  });

  it("throws when accepted is missing", () => {
    mockDecodeAuthResponse.mockReturnValue({
      sessionId: 1n,
      reason: "",
    });
    expect(() =>
      decoder.decodeAuthResponse(new Uint8Array([0])),
    ).toThrow(/accepted must be boolean/);
  });

  it("passes through a well-formed AuthResponseData", () => {
    mockDecodeAuthResponse.mockReturnValue({
      sessionId: 42n,
      accepted: true,
      reason: "",
    });
    const result = decoder.decodeAuthResponse(new Uint8Array([0]));
    expect(result.sessionId).toBe(42n);
    expect(result.accepted).toBe(true);
    expect(result.reason).toBe("");
  });

  it("throws a specific array-rejection error when WASM returns an array", () => {
    mockDecodeAuthResponse.mockReturnValue([1n, true, ""]);
    expect(() =>
      decoder.decodeAuthResponse(new Uint8Array([0])),
    ).toThrow(/array|non-object/);
  });
});
