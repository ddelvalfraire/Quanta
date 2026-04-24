/**
 * H-1: Unvalidated WASM `any` casts
 *
 * These tests prove that `decodeState` and `decodeAuthResponse` in
 * packages/delta-decoder/src/index.ts perform bare `as DecodedState` /
 * `as AuthResponseData` casts on whatever the WASM module returns — with
 * zero runtime validation.  A malformed object flows straight through and
 * is silently returned as if it were valid.
 *
 * Each test mocks the underlying WASM module so that the relevant function
 * returns a structurally wrong value, then calls the public wrapper and
 * asserts that a validation error is thrown BEFORE the bad data reaches
 * the caller.
 *
 * EXPECTED STATUS TODAY: all three tests FAIL because no validation exists.
 * The malformed objects pass through the `as X` cast and are returned to
 * the caller unchanged — no error is thrown.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

// ---------------------------------------------------------------------------
// Module-level mock.
//
// vi.mock is hoisted to the top of the file by vitest's transform.  The mock
// path must match the *resolved* specifier that src/index.ts uses when it
// does `import("../wasm/quanta_wasm_decoder.js")`.  Because index.ts lives
// in src/, that resolves to  packages/delta-decoder/wasm/quanta_wasm_decoder.js.
// From the test file in src/__tests__/ the same physical file is reached via
// "../../wasm/quanta_wasm_decoder.js".  We use the absolute path so there is
// no ambiguity about which copy of the module vitest intercepts.
// ---------------------------------------------------------------------------

const mockDecodeState = vi.fn();
const mockDecodeAuthResponse = vi.fn();

vi.mock(
  "/Users/daviddelval/code/personal/Quanta/packages/delta-decoder/wasm/quanta_wasm_decoder.js",
  () => ({
    // `default` is the WASM init function; we make it a no-op so loadDecoder
    // never tries to instantiate a real WebAssembly binary.
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

// loadDecoder is imported after the mock is hoisted into place.
import { loadDecoder } from "../index.js";

let decoder: Awaited<ReturnType<typeof loadDecoder>>;

beforeEach(async () => {
  vi.clearAllMocks();
  decoder = await loadDecoder(new Uint8Array(0));
});

// ---------------------------------------------------------------------------
// H-1a: decode_state returns wrong shape
// ---------------------------------------------------------------------------

describe("H-1a: decodeState with malformed WASM output", () => {
  it("throws a validation error when WASM decode_state returns an object with wrong fields", () => {
    // Arrange: decode_state returns a structurally wrong object.
    // None of the expected `Record<string, number | boolean>` keys are
    // present; instead a single unrelated key `wrongField` exists.
    mockDecodeState.mockReturnValue({ wrongField: 123 });

    const fakeSchema = {} as Parameters<typeof decoder.decodeState>[0];

    // Act + Assert: the wrapper MUST throw before returning the bad data.
    //
    // FAILS TODAY: no validation exists.  decodeState returns
    // { wrongField: 123 } without throwing, so toThrow() fails and
    // vitest reports this test as FAILED.
    expect(() =>
      decoder.decodeState(fakeSchema, new Uint8Array([0])),
    ).toThrow();
  });
});

// ---------------------------------------------------------------------------
// H-1b: decode_auth_response returns sessionId as string, not bigint
// ---------------------------------------------------------------------------

describe("H-1b: decodeAuthResponse with sessionId as string instead of bigint", () => {
  it("throws a validation error when WASM decode_auth_response returns sessionId as a string", () => {
    // Arrange: mock returns sessionId as a plain string.
    // Per the AuthResponseData contract, sessionId MUST be a bigint.
    // Downstream consumers that do `sessionId + 1n` or `sessionId === 42n`
    // will get a TypeError far from this call site.  The wrapper must
    // reject the bad data here at the boundary.
    mockDecodeAuthResponse.mockReturnValue({
      sessionId: "not a bigint", // should be bigint per AuthResponseData
      accepted: true,
      reason: "",
    });

    // FAILS TODAY: the bare `as AuthResponseData` cast accepts any shape.
    // The string "not a bigint" is returned as-is typed as AuthResponseData,
    // so no error is thrown and toThrow() fails.
    expect(() =>
      decoder.decodeAuthResponse(new Uint8Array([0])),
    ).toThrow();
  });

  it("sessionId is not a bigint when WASM returns a string — downstream bigint arithmetic breaks silently", () => {
    // Secondary assertion: verify the type contract is violated at the
    // boundary.  The wrapper must guarantee typeof sessionId === "bigint".
    //
    // FAILS TODAY: typeof result.sessionId === "string", not "bigint".
    // The `as AuthResponseData` cast lies to the compiler; at runtime the
    // value is a plain string.
    mockDecodeAuthResponse.mockReturnValue({
      sessionId: "not a bigint",
      accepted: true,
      reason: "",
    });

    const result = decoder.decodeAuthResponse(new Uint8Array([0]));

    expect(typeof result.sessionId).toBe("bigint");
  });
});
