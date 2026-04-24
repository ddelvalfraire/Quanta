/**
 * H-3: Unbounded reconnect loop
 *
 * The Connection class schedules reconnects indefinitely when the server is
 * permanently unreachable. There is no maxReconnectAttempts option, no cap on
 * the number of attempts, and no "abandoned" event or observable state that
 * lets the consumer know the loop has given up.
 *
 * These tests FAIL today because:
 *   1. `ConnectionOptions` has no `maxReconnectAttempts` field.
 *   2. `ConnectionEvents` has no `abandoned` event.
 *   3. The reconnect loop in `scheduleReconnect()` runs forever with no exit
 *      condition based on attempt count.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { Connection } from "../connection.js";
import type { ConnectionOptions } from "../connection.js";
import type { Transport, Stream } from "../transport.js";
import { createMockDecoder } from "./helpers.js";

// ---------------------------------------------------------------------------
// A transport that always fails to connect — simulates a permanently
// unreachable server.
// ---------------------------------------------------------------------------

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

  // Inject the always-failing transport as the WebSocket implementation
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  (conn as any).connectWebSocket = async () => {
    throw new Error("server unreachable");
  };

  return conn;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("Connection — H-3: Unbounded reconnect loop", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("FAILS today: ConnectionOptions has no maxReconnectAttempts field", () => {
    // This test verifies the public API surface. The options type does not
    // expose maxReconnectAttempts, so there is no way for a consumer to set a
    // cap. The assertion fails because the property does not exist on the type.
    const opts: ConnectionOptions = {
      url: "https://localhost:4433",
      apiKey: "tok_test",
    };

    // Cast to record to probe for the field that SHOULD exist but does not
    const hasMaxAttempts =
      "maxReconnectAttempts" in (opts as Record<string, unknown>);

    // This assertion FAILS today — the field is absent
    expect(hasMaxAttempts).toBe(true);
  });

  it("FAILS today: no 'abandoned' event fires after exhausting reconnect attempts", async () => {
    // Build a connection with a max of 3 attempts (option does not exist yet).
    const conn = createFailingConnection({
      // @ts-expect-error — maxReconnectAttempts does not exist on ConnectionOptions yet
      maxReconnectAttempts: 3,
    });

    const abandonedEvents: unknown[] = [];

    // 'abandoned' event does not exist in ConnectionEvents today.
    // TypeScript would reject this, so we cast via any.
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    (conn as any).on("abandoned", (attempts: number) => {
      abandonedEvents.push(attempts);
    });

    // Start the connection (will fail immediately)
    conn.connect().catch(() => {});

    // Advance far enough for 3 reconnect cycles to complete.
    // Each attempt uses exponential backoff starting at ~1000 ms.
    // 3 attempts x 2000 ms ceiling = 6000 ms total headroom, plus the
    // initial connect timeout of 100 ms. We advance 30 s to be safe.
    await vi.advanceTimersByTimeAsync(30_000);

    // FAILS today: the 'abandoned' event never fires because the loop is
    // unbounded and the event type does not exist.
    expect(abandonedEvents).toHaveLength(1);
  });

  it("FAILS today: Connection has no public method to stop the reconnect loop", () => {
    // When a consumer wants to give up reconnecting (e.g. show a permanent
    // offline banner), they need a way to abort the loop short of calling
    // disconnect() which also destroys the session. No such method exists.
    //
    // This is a static API-surface assertion. It fails today because no
    // stopReconnecting() / cancelReconnect() method exists on Connection.
    const conn = createFailingConnection();

    // FAILS today: the method does not exist on the Connection class
    expect(typeof (conn as unknown as Record<string, unknown>)["stopReconnecting"]).toBe(
      "function",
    );
  });
});
