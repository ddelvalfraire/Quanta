// Per-entity state cache: maintains the last authoritative state bytes
// for every entity the client has observed, applying deltas as they arrive.

import {
  apply_delta,
  create_schema,
  decode_delta_datagram,
  decode_state,
  SchemaHandle,
} from "../wasm-decoder/quanta_wasm_decoder";

const FLAG_FULL_STATE = 0x01;

export type EntityFields = Record<string, number>;

export type DecodedDelta = {
  flags: number;
  entitySlot: number;
  tick: bigint;
  payload: Uint8Array;
};

export class WorldState {
  private schema: SchemaHandle;
  private byState: Map<number, Uint8Array> = new Map();

  constructor(schemaBytes: Uint8Array) {
    this.schema = create_schema(schemaBytes);
  }

  ingestDatagram(bytes: Uint8Array): { slot: number; tick: bigint } | null {
    let d: DecodedDelta;
    try {
      d = decode_delta_datagram(bytes) as unknown as DecodedDelta;
    } catch (e) {
      console.warn("drop malformed delta datagram", e);
      return null;
    }
    const prev = this.byState.get(d.entitySlot);
    let next: Uint8Array;
    if ((d.flags & FLAG_FULL_STATE) !== 0) {
      next = d.payload;
    } else if (prev) {
      try {
        next = apply_delta(this.schema, prev, d.payload);
      } catch (e) {
        console.warn("apply_delta failed", e);
        return null;
      }
    } else {
      // Delta without baseline — drop; a later FULL_STATE will reset.
      return null;
    }
    this.byState.set(d.entitySlot, next);
    return { slot: d.entitySlot, tick: d.tick };
  }

  entries(): Array<{ slot: number; fields: EntityFields }> {
    const out: Array<{ slot: number; fields: EntityFields }> = [];
    for (const [slot, state] of this.byState) {
      try {
        const fields = decode_state(this.schema, state) as EntityFields;
        out.push({ slot, fields });
      } catch {
        /* skip entities whose bytes are temporarily malformed */
      }
    }
    return out;
  }

  size(): number {
    return this.byState.size;
  }
}
