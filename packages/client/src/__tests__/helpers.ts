import type { AuthResponseData, QuantaDecoder, SchemaHandle, DecodedState } from "@quanta/delta-decoder";
import type { Transport, Stream } from "../transport.js";

// ---------------------------------------------------------------------------
// MockTransport — simulates WebTransport/WebSocket for tests
// ---------------------------------------------------------------------------

export class MockStream implements Stream {
  sent: Uint8Array[] = [];
  recvQueue: Uint8Array[] = [];

  async send(data: Uint8Array): Promise<void> {
    this.sent.push(data);
  }

  async recv(): Promise<Uint8Array> {
    const item = this.recvQueue.shift();
    if (!item) throw new Error("MockStream: recv queue empty");
    return item;
  }

  close(): void {
    // no-op
  }

  /** Enqueue data for the next recv() call. */
  enqueueRecv(data: Uint8Array): void {
    this.recvQueue.push(data);
  }
}

export class MockTransport implements Transport {
  connected = false;
  datagramsSent: Uint8Array[] = [];
  onDatagram: ((data: Uint8Array) => void) | null = null;
  onClose: ((code: number, reason: string) => void) | null = null;
  onError: ((err: unknown) => void) | null = null;

  private streams: MockStream[] = [];

  async openStream(): Promise<Stream> {
    const stream = new MockStream();
    this.streams.push(stream);
    return stream;
  }

  async acceptStream(): Promise<Stream> {
    return this.openStream();
  }

  sendDatagram(data: Uint8Array): void {
    this.datagramsSent.push(data);
  }

  close(): void {
    this.connected = false;
    this.onClose?.(1000, "normal");
  }

  /** Simulate an incoming datagram from the server. */
  simulateDatagram(data: Uint8Array): void {
    this.onDatagram?.(data);
  }

  /** Simulate transport disconnection. */
  simulateClose(code = 1006, reason = "connection lost"): void {
    this.connected = false;
    this.onClose?.(code, reason);
  }

  /** Get the last opened stream for inspection. */
  lastStream(): MockStream | undefined {
    return this.streams[this.streams.length - 1];
  }
}

// ---------------------------------------------------------------------------
// MockDecoder — simulates WASM decoder for tests
// ---------------------------------------------------------------------------

let nextSchemaId = 1;

export class MockSchemaHandle {
  readonly id: number;
  constructor() {
    this.id = nextSchemaId++;
  }
  free(): void {
    // no-op
  }
  get field_count(): number {
    return 2;
  }
  get version(): number {
    return 1;
  }
  get total_bits(): number {
    return 16;
  }
}

export function createMockDecoder(): QuantaDecoder {
  return {
    createSchema(_bytes: Uint8Array): SchemaHandle {
      return new MockSchemaHandle() as unknown as SchemaHandle;
    },

    applyDelta(_schema: SchemaHandle, state: Uint8Array, delta: Uint8Array): Uint8Array {
      // Simple mock: XOR delta onto state
      const result = new Uint8Array(state.length);
      for (let i = 0; i < state.length; i++) {
        result[i] = state[i] ^ (delta[i] ?? 0);
      }
      return result;
    },

    decodeState(_schema: SchemaHandle, state: Uint8Array): DecodedState {
      // Mock: return first two bytes as x/y
      return { x: state[0] ?? 0, y: state[1] ?? 0 };
    },

    encodeState(_schema: SchemaHandle, stateObj: DecodedState): Uint8Array {
      const x = typeof stateObj.x === "number" ? stateObj.x : 0;
      const y = typeof stateObj.y === "number" ? stateObj.y : 0;
      return new Uint8Array([x, y]);
    },

    encodeAuthRequest(token: string, clientVersion: string, sessionToken?: bigint | null): Uint8Array {
      // Mock: encode as JSON prefixed with 4-byte length
      const json = JSON.stringify({
        token,
        clientVersion,
        sessionToken: sessionToken != null ? Number(sessionToken) : null,
      });
      const payload = new TextEncoder().encode(json);
      const buf = new Uint8Array(4 + payload.length);
      new DataView(buf.buffer).setUint32(0, payload.length);
      buf.set(payload, 4);
      return buf;
    },

    decodeAuthResponse(bytes: Uint8Array): AuthResponseData {
      const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
      const len = view.getUint32(0);
      const json = new TextDecoder().decode(bytes.slice(4, 4 + len));
      const parsed = JSON.parse(json);
      return {
        sessionId: BigInt(parsed.sessionId),
        accepted: parsed.accepted,
        reason: parsed.reason,
      };
    },

    encodeAuthResponse(sessionId: bigint, accepted: boolean, reason: string): Uint8Array {
      const json = JSON.stringify({ sessionId: Number(sessionId), accepted, reason });
      const payload = new TextEncoder().encode(json);
      const buf = new Uint8Array(4 + payload.length);
      new DataView(buf.buffer).setUint32(0, payload.length);
      buf.set(payload, 4);
      return buf;
    },

    encodeBaselineAck(baselineTick: bigint): Uint8Array {
      const json = JSON.stringify({ baselineTick: Number(baselineTick) });
      const payload = new TextEncoder().encode(json);
      const buf = new Uint8Array(4 + payload.length);
      new DataView(buf.buffer).setUint32(0, payload.length);
      buf.set(payload, 4);
      return buf;
    },
  };
}

// ---------------------------------------------------------------------------
// Helpers for building mock server messages
// ---------------------------------------------------------------------------

/** Build a mock AuthResponse wire message (length-prefixed JSON for mock decoder). */
export function buildMockAuthResponse(resp: {
  sessionId: number;
  accepted: boolean;
  reason: string;
}): Uint8Array {
  const json = JSON.stringify(resp);
  const payload = new TextEncoder().encode(json);
  const buf = new Uint8Array(4 + payload.length);
  new DataView(buf.buffer).setUint32(0, payload.length);
  buf.set(payload, 4);
  return buf;
}

/** Build a mock InitialStateMessage in the real binary wire format. */
export function buildInitialStateMessage(opts: {
  baselineTick: bigint;
  schemaVersion: number;
  compiledSchema?: Uint8Array;
  entities: Array<{ entitySlot: number; state: Uint8Array }>;
}): Uint8Array {
  const flags = opts.compiledSchema ? 0x01 : 0x00;
  const schemaSection = opts.compiledSchema
    ? 4 + opts.compiledSchema.length
    : 0;
  const entitySize = opts.entities.reduce((acc, e) => acc + 8 + e.state.length, 0);
  const totalSize = 8 + 1 + 1 + schemaSection + 4 + entitySize;

  const buf = new Uint8Array(totalSize);
  const view = new DataView(buf.buffer);
  let pos = 0;

  // baseline_tick: u64 BE
  view.setBigUint64(pos, opts.baselineTick);
  pos += 8;

  // flags: u8
  buf[pos++] = flags;

  // schema_version: u8
  buf[pos++] = opts.schemaVersion;

  // optional schema
  if (opts.compiledSchema) {
    view.setUint32(pos, opts.compiledSchema.length);
    pos += 4;
    buf.set(opts.compiledSchema, pos);
    pos += opts.compiledSchema.length;
  }

  // entity_count: u32 BE
  view.setUint32(pos, opts.entities.length);
  pos += 4;

  for (const entity of opts.entities) {
    view.setUint32(pos, entity.entitySlot);
    pos += 4;
    view.setUint32(pos, entity.state.length);
    pos += 4;
    buf.set(entity.state, pos);
    pos += entity.state.length;
  }

  return buf;
}

/** Wrap a payload with a 4-byte BE length prefix (stream framing). */
export function lengthPrefix(payload: Uint8Array): Uint8Array {
  const buf = new Uint8Array(4 + payload.length);
  new DataView(buf.buffer).setUint32(0, payload.length);
  buf.set(payload, 4);
  return buf;
}
