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
      decodeState: (schema, state) =>
        wasm.decode_state(schema, state) as DecodedState,
      encodeState: (schema, stateObj) =>
        wasm.encode_state(schema, stateObj),
      encodeAuthRequest: (token, clientVersion, sessionToken, transferToken) =>
        wasm.encode_auth_request(token, clientVersion, sessionToken ?? undefined, transferToken ?? undefined),
      decodeAuthResponse: (bytes) =>
        wasm.decode_auth_response(bytes) as AuthResponseData,
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
