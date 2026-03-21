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

/** Interface wrapping the raw WASM exports into an ergonomic API. */
export interface QuantaDecoder {
  createSchema(bytes: Uint8Array): SchemaHandle;
  applyDelta(schema: SchemaHandle, state: Uint8Array, delta: Uint8Array): Uint8Array;
  decodeState(schema: SchemaHandle, state: Uint8Array): DecodedState;
  encodeState(schema: SchemaHandle, stateObj: DecodedState): Uint8Array;
}

let cached: QuantaDecoder | null = null;
let pending: Promise<QuantaDecoder> | null = null;

/**
 * Load and initialize the WASM module, returning a `QuantaDecoder`.
 *
 * The WASM binary is loaded once; subsequent calls return the cached instance.
 */
export async function loadDecoder(): Promise<QuantaDecoder> {
  if (cached) return cached;
  if (pending) return pending;

  pending = (async () => {
    const wasm = await import("../wasm/quanta_wasm_decoder.js");
    await wasm.default();

    const decoder: QuantaDecoder = {
      createSchema: (bytes) => wasm.create_schema(bytes),
      applyDelta: (schema, state, delta) =>
        wasm.apply_delta(schema, state, delta),
      decodeState: (schema, state) =>
        wasm.decode_state(schema, state) as DecodedState,
      encodeState: (schema, stateObj) =>
        wasm.encode_state(schema, stateObj),
    };

    cached = decoder;
    return decoder;
  })();

  return pending;
}
