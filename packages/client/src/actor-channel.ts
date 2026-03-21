import type { QuantaDecoder, SchemaHandle, DecodedState } from "@quanta/delta-decoder";
import type { ActorChannelEvents, EventName } from "./types.js";

/**
 * Per-actor channel wrapper.
 *
 * Manages the binary state for a single actor, applies incoming deltas,
 * and emits typed events. Designed to be used with a Phoenix Channel
 * transport (provided by QuantaClient).
 */
export class ActorChannel {
  readonly namespace: string;
  readonly actorType: string;
  readonly actorId: string;
  readonly topic: string;

  private decoder: QuantaDecoder;
  private schema: SchemaHandle;
  private state: Uint8Array;
  private decoded: DecodedState;
  private seq = 0;
  private listeners = new Map<EventName, Set<(...args: unknown[]) => void>>();

  constructor(
    namespace: string,
    actorType: string,
    actorId: string,
    decoder: QuantaDecoder,
    schema: SchemaHandle,
    initialState: Uint8Array,
  ) {
    this.namespace = namespace;
    this.actorType = actorType;
    this.actorId = actorId;
    this.topic = `actor:${namespace}:${actorType}:${actorId}`;
    this.decoder = decoder;
    this.schema = schema;
    this.state = initialState;
    this.decoded = decoder.decodeState(schema, initialState);
  }

  /** Current decoded state. */
  getState(): DecodedState {
    return this.decoded;
  }

  /** Current raw state bytes. */
  getRawState(): Uint8Array {
    return this.state;
  }

  /** Current sequence number (incremented on each delta). */
  getSeq(): number {
    return this.seq;
  }

  /**
   * Apply a binary delta received from the server.
   * Emits a "delta" event with the new decoded state.
   */
  applyDelta(delta: Uint8Array): void {
    const prev = this.decoded;
    try {
      this.state = this.decoder.applyDelta(this.schema, this.state, delta);
      this.decoded = this.decoder.decodeState(this.schema, this.state);
      this.seq++;

      const changed: string[] = [];
      for (const key of Object.keys(this.decoded)) {
        if (this.decoded[key] !== prev[key]) {
          changed.push(key);
        }
      }

      this.emit("delta", this.decoded, changed, this.seq);
    } catch (err) {
      this.emit("error", err instanceof Error ? err : new Error(String(err)));
    }
  }

  /** Handle server-side node draining signal. */
  handleDraining(reconnectMs: number): void {
    this.emit("draining", reconnectMs);
  }

  /** Handle actor stopped signal. */
  handleStopped(): void {
    this.emit("stopped");
  }

  /** Subscribe to channel events. */
  on<E extends EventName>(event: E, callback: ActorChannelEvents[E]): void {
    let set = this.listeners.get(event);
    if (!set) {
      set = new Set();
      this.listeners.set(event, set);
    }
    set.add(callback as (...args: unknown[]) => void);
  }

  /** Unsubscribe from channel events. */
  off<E extends EventName>(event: E, callback: ActorChannelEvents[E]): void {
    this.listeners.get(event)?.delete(callback as (...args: unknown[]) => void);
  }

  private emit(event: EventName, ...args: unknown[]): void {
    const set = this.listeners.get(event);
    if (!set) return;
    for (const cb of set) {
      try {
        cb(...args);
      } catch {
        // Don't let listener errors break the channel
      }
    }
  }
}
