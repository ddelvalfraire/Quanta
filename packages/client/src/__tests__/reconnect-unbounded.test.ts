import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { Connection } from "../connection.js";
import type { ConnectionOptions } from "../connection.js";
import type { Transport, Stream } from "../transport.js";
import { createMockDecoder } from "./helpers.js";

class AlwaysFailTransport implements Transport {
  connected = false;
  onDatagram: ((data: Uint8Array) => void) | null = null;
  onClose: ((code: number, reason: string) => void) | null = null;
  onError: ((err: unknown) => void) | null = null;

  async openStream(): Promise<Stream> {
    throw new Error("server unreachable");
  }

  async acceptStream(): Promise<Stream> {
    throw new Error("server unreachable");
  }

  sendDatagram(_data: Uint8Array): void {}

  close(): void {
    this.connected = false;
  }
}

function createFailingConnection(
  overrides?: Partial<ConnectionOptions>,
): Connection {
  const decoder = createMockDecoder();

  const opts: ConnectionOptions = {
    url: "https://localhost:4433",
    apiKey: "tok_test",
    forceWebSocket: true,
    connectTimeoutMs: 100,
    ...overrides,
  };

  const conn = new Connection(opts, decoder);

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  (conn as any).connectWebSocket = async () => {
    throw new Error("server unreachable");
  };

  return conn;
}

describe("Connection — H-3: bounded reconnect loop", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("ConnectionOptions accepts maxReconnectAttempts", () => {
    // Compile-time: if the field were missing from ConnectionOptions, this
    // assignment would be a TS error. Runtime: the value survives construction.
    const opts: ConnectionOptions = {
      url: "https://localhost:4433",
      apiKey: "tok_test",
      maxReconnectAttempts: 5,
    };
    expect(opts.maxReconnectAttempts).toBe(5);
  });

  it("fires 'abandoned' event after exhausting reconnect attempts", async () => {
    const conn = createFailingConnection({ maxReconnectAttempts: 3 });

    const abandonedEvents: number[] = [];
    conn.on("abandoned", (attempts) => {
      abandonedEvents.push(attempts);
    });

    conn.connect().catch(() => {});

    await vi.advanceTimersByTimeAsync(30_000);

    expect(abandonedEvents).toHaveLength(1);
  });

  it("exposes stopReconnecting() on Connection", () => {
    const conn = createFailingConnection();
    expect(typeof (conn as unknown as Record<string, unknown>)["stopReconnecting"]).toBe(
      "function",
    );
  });

  it("stopReconnecting() interrupts a scheduled retry mid-flight", async () => {
    const conn = createFailingConnection({ maxReconnectAttempts: 10 });
    const abandonedEvents: number[] = [];
    const reconnectingEvents: number[] = [];
    conn.on("abandoned", (n) => abandonedEvents.push(n));
    conn.on("reconnecting", (n) => reconnectingEvents.push(n));

    conn.connect().catch(() => {});

    // Let a few retry attempts fire
    await vi.advanceTimersByTimeAsync(5000);
    const attemptsBeforeStop = reconnectingEvents.length;

    conn.stopReconnecting();

    // Advance significantly — no more retry attempts should fire
    await vi.advanceTimersByTimeAsync(30_000);

    expect(reconnectingEvents.length).toBe(attemptsBeforeStop);
    expect(abandonedEvents).toHaveLength(0); // Not "abandoned" — user stopped it
  });
});
