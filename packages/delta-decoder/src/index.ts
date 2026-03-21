/**
 * @quanta/delta-decoder
 *
 * Thin TypeScript wrapper around the quanta-wasm-decoder WASM module.
 * Provides `loadDecoder()` which initializes the WASM singleton and returns
 * a `QuantaDecoder` interface for working with binary delta state.
 */

// Re-export the SchemaHandle type from the WASM module's generated typings.
// At runtime this is an opaque wasm-bindgen handle; callers only need the type.
export type { SchemaHandle } from "../wasm/quanta_wasm_decoder.js";

/** Decoded state: a record of field names to their JS-native values. */
export type DecodedState = Record<string, number | boolean>;

/** Interface wrapping the raw WASM exports into an ergonomic API. */
export interface QuantaDecoder {
  /** Parse QSCH binary bytes into a reusable SchemaHandle. */
  createSchema(bytes: Uint8Array): import("../wasm/quanta_wasm_decoder.js").SchemaHandle;

  /** Apply a binary delta to the current state, returning new state bytes. */
  applyDelta(
    schema: import("../wasm/quanta_wasm_decoder.js").SchemaHandle,
    state: Uint8Array,
    delta: Uint8Array,
  ): Uint8Array;

  /** Decode packed state bytes into a JS object `{ fieldName: value }`. */
  decodeState(
    schema: import("../wasm/quanta_wasm_decoder.js").SchemaHandle,
    state: Uint8Array,
  ): DecodedState;

  /** Encode a JS object `{ fieldName: value }` into packed state bytes. */
  encodeState(
    schema: import("../wasm/quanta_wasm_decoder.js").SchemaHandle,
    stateObj: DecodedState,
  ): Uint8Array;
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
        wasm.wasm_apply_delta(schema, state, delta),
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
