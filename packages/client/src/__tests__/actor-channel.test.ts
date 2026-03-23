import { describe, it, expect, vi } from "vitest";
import { ActorChannel } from "../actor-channel.js";
import { createMockDecoder, MockSchemaHandle } from "./helpers.js";
import type { Connection } from "../connection.js";

function createChannel(opts?: {
  connection?: Connection | null;
  state?: Uint8Array;
}) {
  const decoder = createMockDecoder();
  const schema = new MockSchemaHandle();
  const state = opts?.state ?? new Uint8Array([10, 20]);
  const conn = opts?.connection ?? null;

  return new ActorChannel(
    0, // entitySlot
    "test",
    "player",
    "p1",
    decoder,
    schema as any,
    1,
    state,
    conn as any,
  );
}

describe("ActorChannel", () => {
  it("initializes with decoded state", () => {
    const ch = createChannel({ state: new Uint8Array([5, 15]) });
    expect(ch.getState()).toEqual({ x: 5, y: 15 });
    expect(ch.getRawState()).toEqual(new Uint8Array([5, 15]));
    expect(ch.getSeq()).toBe(0);
    expect(ch.getSchemaVersion()).toBe(1);
  });

  it("sets topic correctly", () => {
    const ch = createChannel();
    expect(ch.topic).toBe("actor:test:player:p1");
  });

  it("applies delta and emits event", () => {
    const ch = createChannel({ state: new Uint8Array([10, 20]) });
    const onDelta = vi.fn();
    ch.on("delta", onDelta);

    // Mock decoder XORs delta onto state
    ch.applyDelta(new Uint8Array([0x05, 0x00]));

    expect(ch.getSeq()).toBe(1);
    expect(onDelta).toHaveBeenCalledOnce();

    const [state, changed, seq] = onDelta.mock.calls[0];
    expect(seq).toBe(1);
    // state[0] = 10 ^ 5 = 15, state[1] = 20 ^ 0 = 20
    expect(state.x).toBe(15);
    expect(changed).toContain("x");
  });

  it("emits error on bad delta", () => {
    const decoder = createMockDecoder();
    // Override applyDelta to throw
    decoder.applyDelta = () => {
      throw new Error("corrupt delta");
    };

    const schema = new MockSchemaHandle();
    const ch = new ActorChannel(
      0, "test", "player", "p1",
      decoder, schema as any, 1,
      new Uint8Array([1, 2]), null as any,
    );

    const onError = vi.fn();
    ch.on("error", onError);

    ch.applyDelta(new Uint8Array([0xff]));

    expect(onError).toHaveBeenCalledOnce();
    expect(onError.mock.calls[0][0]).toBeInstanceOf(Error);
    expect(onError.mock.calls[0][0].message).toBe("corrupt delta");
  });

  it("handles snapshot and emits fullState", () => {
    const ch = createChannel({ state: new Uint8Array([1, 2]) });
    const onFullState = vi.fn();
    ch.on("fullState", onFullState);

    ch.handleSnapshot(new Uint8Array([50, 60]), 2, 100);

    expect(ch.getState()).toEqual({ x: 50, y: 60 });
    expect(ch.getSeq()).toBe(100);
    expect(ch.getSchemaVersion()).toBe(2);
    expect(onFullState).toHaveBeenCalledOnce();
    expect(onFullState.mock.calls[0][0]).toEqual({ x: 50, y: 60 });
  });

  it("emits draining event", () => {
    const ch = createChannel();
    const onDraining = vi.fn();
    ch.on("draining", onDraining);

    ch.handleDraining(5000);
    expect(onDraining).toHaveBeenCalledWith(5000);
  });

  it("emits stopped event", () => {
    const ch = createChannel();
    const onStopped = vi.fn();
    ch.on("stopped", onStopped);

    ch.handleStopped();
    expect(onStopped).toHaveBeenCalledOnce();
  });

  it("send() frames payload with entity_slot and input_seq", () => {
    const mockConn = { sendDatagram: vi.fn() };
    const ch = createChannel({ connection: mockConn as any });

    ch.send(new Uint8Array([1, 2, 3]));

    expect(mockConn.sendDatagram).toHaveBeenCalledOnce();
    const frame = mockConn.sendDatagram.mock.calls[0][0] as Uint8Array;
    const view = new DataView(frame.buffer, frame.byteOffset, frame.byteLength);
    // entity_slot = 0 (from createChannel default)
    expect(view.getUint32(0)).toBe(0);
    // input_seq = 0 (first send)
    expect(view.getUint32(4)).toBe(0);
    // payload
    expect(frame.subarray(8)).toEqual(new Uint8Array([1, 2, 3]));
  });

  it("send() auto-increments input_seq", () => {
    const mockConn = { sendDatagram: vi.fn() };
    const ch = createChannel({ connection: mockConn as any });

    ch.send(new Uint8Array([1]));
    ch.send(new Uint8Array([2]));

    const frame0 = mockConn.sendDatagram.mock.calls[0][0] as Uint8Array;
    const frame1 = mockConn.sendDatagram.mock.calls[1][0] as Uint8Array;
    expect(new DataView(frame0.buffer).getUint32(4)).toBe(0);
    expect(new DataView(frame1.buffer).getUint32(4)).toBe(1);
  });

  it("send() throws when not connected", () => {
    const ch = createChannel({ connection: null });
    expect(() => ch.send(new Uint8Array([1]))).toThrow("not connected");
  });

  it("disconnect() clears connection and listeners", () => {
    const ch = createChannel();
    const onDelta = vi.fn();
    ch.on("delta", onDelta);

    ch.disconnect();

    // Listener should be cleared
    ch.applyDelta(new Uint8Array([1, 0]));
    expect(onDelta).not.toHaveBeenCalled();
  });

  it("off() removes specific listener", () => {
    const ch = createChannel();
    const cb1 = vi.fn();
    const cb2 = vi.fn();
    ch.on("delta", cb1);
    ch.on("delta", cb2);

    ch.off("delta", cb1);
    ch.applyDelta(new Uint8Array([1, 0]));

    expect(cb1).not.toHaveBeenCalled();
    expect(cb2).toHaveBeenCalledOnce();
  });
});
