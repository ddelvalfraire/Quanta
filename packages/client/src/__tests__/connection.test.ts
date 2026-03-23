import { describe, it, expect, vi, beforeEach } from "vitest";
import { Connection } from "../connection.js";
import type { ConnectionOptions } from "../connection.js";
import type { Transport, Stream } from "../transport.js";
import {
  createMockDecoder,
  buildMockAuthResponse,
  buildInitialStateMessage,
  lengthPrefix,
  MockStream,
} from "./helpers.js";

// ---------------------------------------------------------------------------
// Helpers — build a Connection with a pluggable transport factory
// ---------------------------------------------------------------------------

class TestableConnection extends Connection {
  mockTransport: TestTransport;

  constructor(opts: ConnectionOptions, transport: TestTransport) {
    const decoder = createMockDecoder();
    // Force WebSocket to avoid WebTransport detection in Node
    super({ ...opts, forceWebSocket: true }, decoder);
    this.mockTransport = transport;
  }
}

/**
 * A test transport that pre-queues auth response + initial state message
 * on its streams, so the connection handshake completes.
 */
class TestTransport implements Transport {
  connected = true;
  datagramsSent: Uint8Array[] = [];
  onDatagram: ((data: Uint8Array) => void) | null = null;
  onClose: ((code: number, reason: string) => void) | null = null;
  onError: ((err: unknown) => void) | null = null;
  streams: MockStream[] = [];

  /** Pre-built auth response and initial state message for handshake. */
  authResponse: Uint8Array;
  initialState: Uint8Array;

  constructor() {
    this.authResponse = buildMockAuthResponse({
      sessionId: 1,
      accepted: true,
      reason: "",
    });

    const stateMsg = buildInitialStateMessage({
      baselineTick: 100n,
      schemaVersion: 1,
      compiledSchema: new Uint8Array([0x01]),
      entities: [{ entitySlot: 0, state: new Uint8Array([10, 20]) }],
    });
    this.initialState = lengthPrefix(stateMsg);
  }

  async openStream(): Promise<Stream> {
    const stream = new MockStream();
    // Auth stream — client-initiated
    stream.enqueueRecv(this.authResponse);
    this.streams.push(stream);
    return stream;
  }

  async acceptStream(): Promise<Stream> {
    const stream = new MockStream();
    // Sync stream — server-initiated
    stream.enqueueRecv(this.initialState);
    this.streams.push(stream);
    return stream;
  }

  sendDatagram(data: Uint8Array): void {
    this.datagramsSent.push(data);
  }

  close(): void {
    this.connected = false;
    this.onClose?.(1000, "normal");
  }

  simulateClose(code = 1006, reason = "lost"): void {
    this.connected = false;
    this.onClose?.(code, reason);
  }

  simulateDatagram(data: Uint8Array): void {
    this.onDatagram?.(data);
  }
}

/**
 * Create a Connection that uses a TestTransport instead of real WebSocket/WebTransport.
 * We monkey-patch the internal connectWebSocket to return our test transport.
 */
function createTestConnection(
  overrides?: Partial<ConnectionOptions>,
): { conn: Connection; transport: TestTransport } {
  const transport = new TestTransport();
  const decoder = createMockDecoder();

  const opts: ConnectionOptions = {
    url: "https://localhost:4433",
    apiKey: "tok_test",
    forceWebSocket: true,
    ...overrides,
  };

  const conn = new Connection(opts, decoder);

  // Monkey-patch to inject our test transport
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  (conn as any).connectWebSocket = async () => transport;

  return { conn, transport };
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("Connection", () => {
  it("connects, authenticates, and syncs", async () => {
    const { conn, transport } = createTestConnection();

    const msg = await conn.connect();

    expect(conn.state).toBe("connected");
    expect(conn.getSessionId()).toBe(1n);
    expect(msg.baselineTick).toBe(100n);
    expect(msg.entities).toHaveLength(1);
    expect(msg.entities[0].entitySlot).toBe(0);
  });

  it("emits connected event with initial state", async () => {
    const { conn } = createTestConnection();
    const onConnected = vi.fn();
    conn.on("connected", onConnected);

    await conn.connect();

    expect(onConnected).toHaveBeenCalledOnce();
    const [msg, kind] = onConnected.mock.calls[0];
    expect(msg.baselineTick).toBe(100n);
    expect(kind).toBe("websocket");
  });

  it("forwards datagrams to listeners", async () => {
    const { conn, transport } = createTestConnection();
    await conn.connect();

    const received: Uint8Array[] = [];
    conn.on("datagram", (data) => received.push(data as Uint8Array));

    transport.simulateDatagram(new Uint8Array([42, 43]));

    expect(received).toHaveLength(1);
    expect(received[0]).toEqual(new Uint8Array([42, 43]));
  });

  it("sends datagrams through transport", async () => {
    const { conn, transport } = createTestConnection();
    await conn.connect();

    conn.sendDatagram(new Uint8Array([1, 2, 3]));

    expect(transport.datagramsSent).toHaveLength(1);
    expect(transport.datagramsSent[0]).toEqual(new Uint8Array([1, 2, 3]));
  });

  it("disconnect() closes transport and emits disconnected", async () => {
    const { conn } = createTestConnection();
    await conn.connect();

    const onDisconnected = vi.fn();
    conn.on("disconnected", onDisconnected);

    conn.disconnect();

    expect(conn.state).toBe("disconnected");
    expect(onDisconnected).toHaveBeenCalledWith(1000, "client disconnect");
  });

  it("emits reconnecting on unexpected transport close", async () => {
    vi.useFakeTimers();
    const { conn, transport } = createTestConnection();
    await conn.connect();

    const onReconnecting = vi.fn();
    conn.on("reconnecting", onReconnecting);

    transport.simulateClose(1006, "connection lost");

    expect(conn.state).toBe("reconnecting");
    expect(onReconnecting).toHaveBeenCalledOnce();

    // Clean up
    conn.disconnect();
    vi.useRealTimers();
  });

  it("does not reconnect after intentional disconnect", async () => {
    vi.useFakeTimers();
    const { conn } = createTestConnection();
    await conn.connect();

    const onReconnecting = vi.fn();
    conn.on("reconnecting", onReconnecting);

    conn.disconnect();

    // Advance time — should not trigger reconnection
    await vi.advanceTimersByTimeAsync(60_000);
    expect(onReconnecting).not.toHaveBeenCalled();

    vi.useRealTimers();
  });

  it("reports websocket transport kind", async () => {
    const { conn } = createTestConnection();
    await conn.connect();
    expect(conn.getTransportKind()).toBe("websocket");
  });

  it("on/off removes listener", async () => {
    const { conn, transport } = createTestConnection();
    await conn.connect();

    const cb = vi.fn();
    conn.on("datagram", cb);
    conn.off("datagram", cb);

    transport.simulateDatagram(new Uint8Array([1]));
    expect(cb).not.toHaveBeenCalled();
  });
});
