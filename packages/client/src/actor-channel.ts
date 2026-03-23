import type { QuantaDecoder, SchemaHandle, DecodedState } from "@quanta/delta-decoder";
import type { ActorChannelEvents, EventName } from "./types.js";
import type { Connection } from "./connection.js";
import { TypedEmitter } from "./emitter.js";

export class ActorChannel extends TypedEmitter<ActorChannelEvents> {
  readonly entitySlot: number;
  readonly namespace: string;
  readonly actorType: string;
  readonly actorId: string;
  readonly topic: string;

  private decoder: QuantaDecoder;
  private schema: SchemaHandle;
  private state: Uint8Array;
  private decoded: DecodedState;
  private seq = 0;
  private inputSeq = 0;
  private schemaVersion: number;
  private connection: Connection | null;

  constructor(
    entitySlot: number,
    namespace: string,
    actorType: string,
    actorId: string,
    decoder: QuantaDecoder,
    schema: SchemaHandle,
    schemaVersion: number,
    initialState: Uint8Array,
    connection: Connection | null,
  ) {
    super();
    this.entitySlot = entitySlot;
    this.namespace = namespace;
    this.actorType = actorType;
    this.actorId = actorId;
    this.topic = `actor:${namespace}:${actorType}:${actorId}`;
    this.decoder = decoder;
    this.schema = schema;
    this.schemaVersion = schemaVersion;
    this.state = initialState;
    this.decoded = decoder.decodeState(schema, initialState);
    this.connection = connection;
  }

  getState(): DecodedState {
    return this.decoded;
  }

  getRawState(): Uint8Array {
    return this.state;
  }

  getSeq(): number {
    return this.seq;
  }

  getSchemaVersion(): number {
    return this.schemaVersion;
  }

  /** Frames as [entity_slot:u32 BE][input_seq:u32 BE][payload] per ClientInput. */
  send(payload: Uint8Array): void {
    if (!this.connection) {
      throw new Error("ActorChannel not connected");
    }
    const seq = this.inputSeq++;
    const frame = new Uint8Array(8 + payload.length);
    const view = new DataView(frame.buffer);
    view.setUint32(0, this.entitySlot);
    view.setUint32(4, seq);
    frame.set(payload, 8);
    this.connection.sendDatagram(frame);
  }

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

  handleSnapshot(
    stateBytes: Uint8Array,
    schemaVersion: number,
    seq: number,
  ): void {
    this.state = stateBytes;
    this.schemaVersion = schemaVersion;
    this.seq = seq;
    this.decoded = this.decoder.decodeState(this.schema, stateBytes);
    this.emit("fullState", this.decoded);
  }

  handleDraining(reconnectMs: number): void {
    this.emit("draining", reconnectMs);
  }

  handleStopped(): void {
    this.emit("stopped");
  }

  disconnect(): void {
    this.connection = null;
    this.clearListeners();
  }
}
