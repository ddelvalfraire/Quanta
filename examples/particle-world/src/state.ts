// Per-entity state cache with tick-time snapshot interpolation.
//
// CRITICAL: interpolation uses the SERVER TICK as the timeline, not the
// local arrival wall-clock. Arrival times include network + OS + JS
// event-loop jitter — feeding that into the lerp fraction makes smooth
// server motion look jittery on screen. Instead we maintain a base
// (baseTick, baseLocalT) anchor and derive a virtual server time from
// `performance.now()`. The anchor is nudged slowly toward each new
// snapshot's (tick, arrival) pair so we absorb jitter without letting
// the virtual clock drift.
//
// References:
//   Gabriel Gambetta — Fast-Paced Multiplayer Part III: Entity Interpolation
//   Valve Source — Source Multiplayer Networking, `cl_interp` 0.1

import {
  apply_delta,
  create_schema,
  decode_delta_datagram,
  decode_state,
  SchemaHandle,
} from "../wasm-decoder/quanta_wasm_decoder";

const FLAG_FULL_STATE = 0x01;
/** Server→client identity datagram carrying the client's own entity slot. */
export const FLAG_WELCOME = 0x02;
/**
 * When set, `payload[0..4]` is a big-endian u32 `last_processed_input_seq`
 * for the receiving client's own entity. Rest of payload is the normal
 * delta/FULL_STATE bytes. See `delta_envelope.rs::FLAG_HAS_SEQ_ACK`.
 */
export const FLAG_HAS_SEQ_ACK = 0x04;
const SEQ_ACK_PREFIX_LEN = 4;

/**
 * Server tick rate in Hz. Must match `DEMO_TICK_RATE_HZ` in
 * `rust/quanta-particle-demo/src/bin/particle-server.rs`. Used to convert
 * ticks ↔ time when interpolating.
 */
export const TICK_RATE_HZ = 30;
export const TICK_PERIOD_MS = 1000 / TICK_RATE_HZ;

/**
 * Interpolation delay in TICKS. The renderer shows the world
 * `INTERP_DELAY_TICKS * TICK_PERIOD_MS` behind the latest received
 * snapshot — long enough that one or two lost packets still leave two
 * valid snapshots to interpolate between. Too short and a single
 * dropped datagram produces a buffer gap → linear interp across N ticks
 * at constant slope → visible velocity discontinuity at the recovery
 * point (shows as a spike in `npc vel cv` on the HUD).
 *
 * 3 ticks at 30 Hz = 100 ms. Gives us 2-packet loss tolerance without
 * adding perceptible extra latency to remote entities (self runs on
 * client-side prediction, so its latency is unaffected).
 */
export const INTERP_DELAY_TICKS = 3;
export const INTERP_DELAY_MS = INTERP_DELAY_TICKS * TICK_PERIOD_MS;

/** How many snapshots to keep per entity. 8 × 33 ms = 264 ms of history. */
const BUFFER_CAPACITY = 8;

/**
 * How aggressively to nudge the `baseLocalT` anchor toward the latest
 * snapshot's arrival time. 0.02 = 2% correction per new snapshot — slow
 * enough that single-packet jitter barely moves the clock, fast enough
 * that over a handful of snapshots we track a drifting server.
 */
const ANCHOR_NUDGE_ALPHA = 0.02;

export type EntityFields = Record<string, number>;

export type DecodedDelta = {
  flags: number;
  entitySlot: number;
  tick: bigint;
  payload: Uint8Array;
};

type Sample = {
  posX: number;
  posZ: number;
  velX: number;
  velZ: number;
  /** Server tick at which this state was produced. */
  tick: number;
  /** Last client input sequence acked by server (self slot only; else 0). */
  lastInputSeq: number;
};

export type InterpEntity = {
  slot: number;
  x: number;
  z: number;
  velX: number;
  velZ: number;
};

export type SelfSnapshot = {
  posX: number;
  posZ: number;
  velX: number;
  velZ: number;
  tick: number;
  /** Last client input sequence the server has processed for this entity. */
  lastInputSeq: number;
};

export class WorldState {
  private schema: SchemaHandle;
  private byState: Map<number, Uint8Array> = new Map();
  private buffers: Map<number, Sample[]> = new Map();

  /** Virtual-clock anchor: "at local time `baseLocalT`, server was at tick `baseTick`." */
  private baseTick: number | null = null;
  private baseLocalT = 0;
  /** Highest tick we've observed from any snapshot. */
  private latestTick = 0;

  constructor(schemaBytes: Uint8Array) {
    this.schema = create_schema(schemaBytes);
  }

  ingestDatagram(
    bytes: Uint8Array,
  ): { slot: number; tick: bigint; flags: number; lastInputSeq: number } | null {
    let d: DecodedDelta;
    try {
      d = decode_delta_datagram(bytes) as unknown as DecodedDelta;
    } catch (e) {
      console.warn("drop malformed delta datagram", e);
      return null;
    }

    // Welcome datagrams are identity-only — no state payload, no tick advance.
    if ((d.flags & FLAG_WELCOME) !== 0) {
      return { slot: d.entitySlot, tick: d.tick, flags: d.flags, lastInputSeq: 0 };
    }

    // Peel off the 4-byte seq-ack prefix when FLAG_HAS_SEQ_ACK is set.
    // The remainder is the normal delta/FULL_STATE payload.
    let lastInputSeq = 0;
    let payload = d.payload;
    if ((d.flags & FLAG_HAS_SEQ_ACK) !== 0) {
      if (payload.length < SEQ_ACK_PREFIX_LEN) {
        console.warn("drop truncated seq-ack datagram", payload.length);
        return null;
      }
      // Big-endian u32 → JS number (seq fits in 32 bits, well under 2^53).
      const b0 = payload[0];
      const b1 = payload[1];
      const b2 = payload[2];
      const b3 = payload[3];
      lastInputSeq = ((b0 << 24) >>> 0) + (b1 << 16) + (b2 << 8) + b3;
      payload = payload.subarray(SEQ_ACK_PREFIX_LEN);
    }

    const tick = Number(d.tick);
    const arrivalLocalT = performance.now();

    // First substantive snapshot seeds the virtual clock.
    //
    // NOTE (experiment reverted): we previously seeded
    // `baseLocalT = arrivalLocalT - INTERP_DELAY_MS` to begin
    // interpolating on frame 1 instead of after a ~100 ms warm-up.
    // That shrank the effective interp-buffer headroom for ~7 s
    // while the anchor nudge re-converged the clock; under any
    // scheduling jitter in that window renderTick clamped to the
    // newest snapshot and produced visible stutter on every tick
    // boundary. Restoring the straight seed: we lose one tick of
    // initial smoothness and gain 7 seconds of stable interp headroom.
    if (this.baseTick === null) {
      this.baseTick = tick;
      this.baseLocalT = arrivalLocalT;
      this.latestTick = tick;
    } else if (tick > this.latestTick) {
      // ONLY nudge the anchor when a new tick arrives. With 300 NPCs at
      // 30 Hz we receive 9000+ snapshots/sec; if the anchor integrated
      // arrival-time jitter from every one, per-packet scheduling jitter
      // compounds into a wobble that manifests as micro-jitter across
      // every interpolated entity. Nudging only on tick advance keeps
      // the clock tracking the server timeline without amplifying
      // per-packet variance.
      const expectedLocalT =
        this.baseLocalT + (tick - this.baseTick) * TICK_PERIOD_MS;
      const drift = arrivalLocalT - expectedLocalT;
      this.baseLocalT += drift * ANCHOR_NUDGE_ALPHA;
      this.latestTick = tick;
    }

    const prev = this.byState.get(d.entitySlot);
    // Seq-ack heartbeat: FLAG_HAS_SEQ_ACK set, FLAG_FULL_STATE clear,
    // empty delta payload. State didn't change, but we need to refresh
    // the buffer sample with the new `lastInputSeq` so `main.ts`'s
    // `reconcile` can drain the predictor's pending-input buffer. The
    // cleanest path: reuse `prev` as the "next" state — decode below
    // will produce a fresh `Sample` with the new seq stamped on it.
    const isHeartbeat =
      (d.flags & FLAG_HAS_SEQ_ACK) !== 0 &&
      (d.flags & FLAG_FULL_STATE) === 0 &&
      payload.length === 0;

    let next: Uint8Array;
    if (isHeartbeat) {
      if (!prev) {
        // No baseline to attach to, but surface the seq-ack upward so
        // callers that only need the ack (not the state) still learn it.
        return { slot: d.entitySlot, tick: d.tick, flags: d.flags, lastInputSeq };
      }
      next = prev;
    } else if ((d.flags & FLAG_FULL_STATE) !== 0) {
      next = payload;
    } else if (prev) {
      try {
        next = apply_delta(this.schema, prev, payload);
      } catch (e) {
        console.warn("apply_delta failed", e);
        return null;
      }
    } else {
      return null;
    }
    this.byState.set(d.entitySlot, next);

    try {
      const fields = decode_state(this.schema, next) as EntityFields;
      const sample: Sample = {
        posX: fields["pos-x"] ?? 0,
        posZ: fields["pos-z"] ?? 0,
        velX: fields["vel-x"] ?? 0,
        velZ: fields["vel-z"] ?? 0,
        tick,
        lastInputSeq,
      };
      let buf = this.buffers.get(d.entitySlot);
      if (!buf) {
        buf = [];
        this.buffers.set(d.entitySlot, buf);
      }
      // Snapshots can theoretically arrive out-of-order under packet
      // reordering; insert while maintaining monotonic tick order.
      if (buf.length === 0 || buf[buf.length - 1].tick < tick) {
        buf.push(sample);
      } else {
        let i = buf.length - 1;
        while (i > 0 && buf[i - 1].tick > tick) i--;
        if (buf[i].tick === tick) buf[i] = sample;
        else buf.splice(i, 0, sample);
      }
      if (buf.length > BUFFER_CAPACITY) buf.shift();
    } catch {
      /* ignore transiently malformed state */
    }
    return { slot: d.entitySlot, tick: d.tick, flags: d.flags, lastInputSeq };
  }

  /** Latest authoritative snapshot for `slot`, for predictor reconciliation. */
  latestSnapshot(slot: number): SelfSnapshot | null {
    const buf = this.buffers.get(slot);
    if (!buf || buf.length === 0) return null;
    return { ...buf[buf.length - 1] };
  }

  /**
   * Compute the virtual "server render tick" at local time `now`. This is
   * the latest known tick minus the interpolation delay. Because the
   * anchor nudges slowly, the resulting tick advances at a rock-steady
   * `1 per TICK_PERIOD_MS` rate regardless of per-packet arrival jitter.
   */
  private renderTickAt(now: number): number {
    if (this.baseTick === null) return 0;
    const virtualTick =
      this.baseTick + (now - this.baseLocalT) / TICK_PERIOD_MS;
    return virtualTick - INTERP_DELAY_TICKS;
  }

  /**
   * Interpolated positions at local time `now`, using the two snapshots
   * that bracket the virtual render tick. `skipSlot` is the self entity,
   * which is rendered from the client-side predictor instead.
   */
  interpolate(now: number, skipSlot: number | null): InterpEntity[] {
    const renderTick = this.renderTickAt(now);
    const out: InterpEntity[] = [];
    for (const [slot, buf] of this.buffers) {
      if (slot === skipSlot) continue;
      if (buf.length === 0) continue;

      if (buf.length === 1 || renderTick <= buf[0].tick) {
        const s = buf[0];
        out.push({ slot, x: s.posX, z: s.posZ, velX: s.velX, velZ: s.velZ });
        continue;
      }
      const newest = buf[buf.length - 1];
      if (renderTick >= newest.tick) {
        out.push({
          slot,
          x: newest.posX,
          z: newest.posZ,
          velX: newest.velX,
          velZ: newest.velZ,
        });
        continue;
      }
      let a = buf[0];
      let b = buf[1];
      for (let i = 1; i < buf.length; i++) {
        if (buf[i].tick >= renderTick) {
          a = buf[i - 1];
          b = buf[i];
          break;
        }
      }
      const span = b.tick - a.tick;
      const alpha = span <= 0 ? 0 : (renderTick - a.tick) / span;
      out.push({
        slot,
        x: a.posX + (b.posX - a.posX) * alpha,
        z: a.posZ + (b.posZ - a.posZ) * alpha,
        velX: a.velX + (b.velX - a.velX) * alpha,
        velZ: a.velZ + (b.velZ - a.velZ) * alpha,
      });
    }
    return out;
  }

  size(): number {
    return this.buffers.size;
  }
}
