/**
 * @quanta/delta-decoder
 *
 * Thin TypeScript wrapper around the quanta-wasm-decoder WASM module.
 * Provides `loadDecoder()` which initializes the WASM singleton and returns
 * a `QuantaDecoder` interface for working with binary delta state.
 */

export type { SchemaHandle } from "../wasm/quanta_wasm_decoder.js";

type SchemaHandle = import("../wasm/quanta_wasm_decoder.js").SchemaHandle;

/** Decoded state: a record of field names to their JS-native values. */
export type DecodedState = Record<string, number | boolean>;

/** Decoded auth response from the realtime server. */
export interface AuthResponseData {
  sessionId: bigint;
  accepted: boolean;
  reason: string;
}

/** Interface wrapping the raw WASM exports into an ergonomic API. */
export interface QuantaDecoder {
  createSchema(bytes: Uint8Array): SchemaHandle;
  applyDelta(schema: SchemaHandle, state: Uint8Array, delta: Uint8Array): Uint8Array;
  decodeState(schema: SchemaHandle, state: Uint8Array): DecodedState;
  encodeState(schema: SchemaHandle, stateObj: DecodedState): Uint8Array;

  /** Encode an AuthRequest to bitcode bytes for the realtime server handshake. */
  encodeAuthRequest(
    token: string,
    clientVersion: string,
    sessionToken?: bigint | null,
    transferToken?: Uint8Array | null,
  ): Uint8Array;
  /** Decode an AuthResponse from bitcode bytes. */
  decodeAuthResponse(bytes: Uint8Array): AuthResponseData;
  /** Encode an AuthResponse to bitcode bytes (for mock servers / testing). */
  encodeAuthResponse(sessionId: bigint, accepted: boolean, reason: string): Uint8Array;
  /** Encode a BaselineAck as [4B len][bitcode]. */
  encodeBaselineAck(baselineTick: bigint): Uint8Array;
}

// wasm-bindgen types the decoder returns as `any`; validate at the boundary.
function assertDecodedState(x: unknown): asserts x is DecodedState {
  if (x === null || typeof x !== "object" || Array.isArray(x)) {
    throw new Error(
      `decode_state returned non-object value: ${x === null ? "null" : Array.isArray(x) ? "array" : typeof x}`,
    );
  }
  for (const [key, value] of Object.entries(x as Record<string, unknown>)) {
    if (typeof value !== "number" && typeof value !== "boolean") {
      throw new Error(
        `decode_state field "${key}" has invalid type ${typeof value}; expected number or boolean`,
      );
    }
  }
}

function assertAuthResponse(x: unknown): asserts x is AuthResponseData {
  if (x === null || typeof x !== "object" || Array.isArray(x)) {
    throw new Error(
      `decode_auth_response returned non-object value: ${x === null ? "null" : Array.isArray(x) ? "array" : typeof x}`,
    );
  }
  const obj = x as Record<string, unknown>;
  if (typeof obj.sessionId !== "bigint") {
    throw new Error(
      `decode_auth_response.sessionId must be bigint; got ${typeof obj.sessionId}`,
    );
  }
  if (typeof obj.accepted !== "boolean") {
    throw new Error(
      `decode_auth_response.accepted must be boolean; got ${typeof obj.accepted}`,
    );
  }
  if (typeof obj.reason !== "string") {
    throw new Error(
      `decode_auth_response.reason must be string; got ${typeof obj.reason}`,
    );
  }
}

let cached: QuantaDecoder | null = null;
let pending: Promise<QuantaDecoder> | null = null;

/**
 * Load and initialize the WASM module, returning a `QuantaDecoder`.
 *
 * The WASM binary is loaded once; subsequent calls return the cached instance.
 *
 * @param wasmInit — Optional: a `BufferSource` or `WebAssembly.Module` to
 *   initialize from. Useful in Node.js where `fetch` of file URLs isn't
 *   supported. In browsers, omit this to auto-resolve via `import.meta.url`.
 */
export async function loadDecoder(
  wasmInit?: BufferSource | WebAssembly.Module,
): Promise<QuantaDecoder> {
  if (cached) return cached;
  if (pending) return pending;

  pending = (async () => {
    const wasm = await import("../wasm/quanta_wasm_decoder.js");
    await wasm.default(wasmInit);

    const decoder: QuantaDecoder = {
      createSchema: (bytes) => wasm.create_schema(bytes),
      applyDelta: (schema, state, delta) =>
        wasm.apply_delta(schema, state, delta),
      decodeState: (schema, state) => {
        const decoded: unknown = wasm.decode_state(schema, state);
        assertDecodedState(decoded);
        return decoded;
      },
      encodeState: (schema, stateObj) =>
        wasm.encode_state(schema, stateObj),
      encodeAuthRequest: (token, clientVersion, sessionToken, transferToken) =>
        wasm.encode_auth_request(token, clientVersion, sessionToken ?? undefined, transferToken ?? undefined),
      decodeAuthResponse: (bytes) => {
        const decoded: unknown = wasm.decode_auth_response(bytes);
        assertAuthResponse(decoded);
        return decoded;
      },
      encodeAuthResponse: (sessionId, accepted, reason) =>
        wasm.encode_auth_response(sessionId, accepted, reason),
      encodeBaselineAck: (baselineTick) =>
        wasm.encode_baseline_ack(baselineTick),
    };

    cached = decoder;
    return decoder;
  })();

  return pending;
}
