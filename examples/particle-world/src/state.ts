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

type EntityTrack = {
  posX: number;
  posZ: number;
  velX: number;
  velZ: number;
  /** `performance.now()` at the moment the server snapshot arrived. */
  t: number;
};

export type InterpEntity = {
  slot: number;
  /** Interpolated world-x at the requested time. */
  x: number;
  /** Interpolated world-z at the requested time. */
  z: number;
  velX: number;
  velZ: number;
};

export class WorldState {
  private schema: SchemaHandle;
  private byState: Map<number, Uint8Array> = new Map();
  private tracks: Map<number, EntityTrack> = new Map();

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
      return null; // No baseline; wait for next FULL_STATE.
    }
    this.byState.set(d.entitySlot, next);

    // Decode once now so the render loop never has to pay the JS-object
    // allocation cost of `decode_state` at 60 fps × 500 entities.
    try {
      const fields = decode_state(this.schema, next) as EntityFields;
      this.tracks.set(d.entitySlot, {
        posX: fields["pos-x"] ?? 0,
        posZ: fields["pos-z"] ?? 0,
        velX: fields["vel-x"] ?? 0,
        velZ: fields["vel-z"] ?? 0,
        t: performance.now(),
      });
    } catch {
      /* ignore transiently malformed state */
    }
    return { slot: d.entitySlot, tick: d.tick };
  }

  /**
   * Interpolated entities at `now` (typically `performance.now()` from
   * the render loop). Uses the last authoritative pos + vel to
   * extrapolate — smooths over the 50 ms gap between 20 Hz ticks.
   */
  interpolate(now: number): InterpEntity[] {
    // Cap extrapolation to ~150 ms so lost packets don't let entities
    // drift off into oblivion on stale velocity.
    const MAX_DT_MS = 150;
    const out: InterpEntity[] = [];
    for (const [slot, tr] of this.tracks) {
      const dt = Math.min(now - tr.t, MAX_DT_MS) / 1000;
      out.push({
        slot,
        x: tr.posX + tr.velX * dt,
        z: tr.posZ + tr.velZ * dt,
        velX: tr.velX,
        velZ: tr.velZ,
      });
    }
    return out;
  }

  size(): number {
    return this.tracks.size;
  }
}
