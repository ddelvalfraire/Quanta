/**
 * H-6: WebTransport datagram reader never cancelled on close()
 *
 * WebTransportAdapter.readDatagrams() acquires a reader lock on
 * wt.datagrams.readable via getReader(). When close() is called, it sets
 * closed=true and calls wt.close(), but never calls reader.cancel() or
 * reader.releaseLock(). The lock is held indefinitely.
 *
 * These tests FAIL today because WebTransportAdapter.close() does not call
 * reader.cancel(). The mock assertion that cancel() was invoked will not pass.
 */

import { describe, it, expect, vi } from "vitest";
import { WebTransportAdapter } from "../transport.js";

// ---------------------------------------------------------------------------
// Minimal WebTransport mock with instrumented datagram reader
// ---------------------------------------------------------------------------

function createMockWebTransport() {
  let cancelCalled = false;

  // The reader object that readDatagrams() will obtain
  const mockReader = {
    read: vi.fn<() => Promise<{ value: Uint8Array | undefined; done: boolean }>>(
      () =>
        // Blocks indefinitely — simulates a quiet stream with no datagrams
        new Promise(() => {}),
    ),
    cancel: vi.fn<() => Promise<void>>(async () => {
      cancelCalled = true;
    }),
    releaseLock: vi.fn(),
  };

  const mockReadable = {
    getReader: vi.fn(() => mockReader),
  };

  const mockWritable = {
    getWriter: vi.fn(() => ({
      write: vi.fn().mockResolvedValue(undefined),
      close: vi.fn().mockResolvedValue(undefined),
    })),
  };

  const mockWt = {
    ready: Promise.resolve(),
    closed: new Promise<{ closeCode: number; reason: string }>(() => {}), // never resolves
    datagrams: {
      readable: mockReadable,
      writable: mockWritable,
    },
    createBidirectionalStream: vi.fn(),
    incomingBidirectionalStreams: { getReader: vi.fn() },
    close: vi.fn(),
  };

  return { mockWt, mockReader, cancelCalled: () => cancelCalled };
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("WebTransportAdapter — H-6: reader.cancel() on close()", () => {
  it("FAILS today: close() does not call reader.cancel() on the datagram reader", async () => {
    const { mockWt, mockReader } = createMockWebTransport();

    const OriginalWebTransport = (globalThis as Record<string, unknown>).WebTransport;
    (globalThis as Record<string, unknown>).WebTransport = vi
      .fn()
      .mockImplementation(() => mockWt);

    try {
      const adapter = new WebTransportAdapter();
      await adapter.connect("https://localhost:4433");

      // Allow readDatagrams() microtask to start and acquire the reader lock
      await new Promise((r) => setTimeout(r, 0));

      // Close the adapter
      adapter.close();

      // Allow any microtasks triggered by close() to settle
      await new Promise((r) => setTimeout(r, 0));

      // FAILS today: close() never calls reader.cancel(), so this assertion
      // fails — cancel is never invoked.
      expect(mockReader.cancel).toHaveBeenCalledOnce();
    } finally {
      (globalThis as Record<string, unknown>).WebTransport = OriginalWebTransport;
    }
  });

  it("FAILS today: calling close() before wt.ready resolves leaves reader lock unreleased", async () => {
    // Race condition: if close() is called before the read loop starts,
    // the reader lock acquired when readDatagrams() eventually runs is
    // never released because close() has already finished without knowing
    // about the reader.
    let resolveReady!: () => void;
    const readyPromise = new Promise<void>((res) => {
      resolveReady = res;
    });

    let cancelCalled = false;
    const mockReader = {
      read: vi.fn<() => Promise<{ value: Uint8Array | undefined; done: boolean }>>(
        () => new Promise(() => {}), // blocks forever
      ),
      cancel: vi.fn<() => Promise<void>>(async () => {
        cancelCalled = true;
      }),
      releaseLock: vi.fn(),
    };

    const mockWt = {
      ready: readyPromise,
      closed: new Promise<{ closeCode: number; reason: string }>(() => {}),
      datagrams: {
        readable: { getReader: vi.fn(() => mockReader) },
        writable: {
          getWriter: vi.fn(() => ({
            write: vi.fn(),
            close: vi.fn().mockResolvedValue(undefined),
          })),
        },
      },
      createBidirectionalStream: vi.fn(),
      incomingBidirectionalStreams: { getReader: vi.fn() },
      close: vi.fn(),
    };

    const OriginalWebTransport = (globalThis as Record<string, unknown>).WebTransport;
    (globalThis as Record<string, unknown>).WebTransport = vi
      .fn()
      .mockImplementation(() => mockWt);

    try {
      const adapter = new WebTransportAdapter();

      // Start connect but do NOT await — ready has not resolved yet
      const connectPromise = adapter.connect("https://localhost:4433");

      // Call close() while still waiting for ready
      adapter.close();

      // Now let ready resolve so the connect path can proceed
      resolveReady();
      await connectPromise.catch(() => {}); // may throw — that is expected

      // Allow all microtasks to settle
      await new Promise((r) => setTimeout(r, 0));

      // FAILS today: reader.cancel() is never called because close() does not
      // track the reader and has no mechanism to cancel it retroactively.
      expect(cancelCalled).toBe(true);
    } finally {
      (globalThis as Record<string, unknown>).WebTransport = OriginalWebTransport;
    }
  });
});
