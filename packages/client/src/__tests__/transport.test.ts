import { describe, it, expect, vi } from "vitest";
import { MockTransport, MockStream } from "./helpers.js";

describe("MockTransport", () => {
  it("tracks sent datagrams", () => {
    const t = new MockTransport();
    t.sendDatagram(new Uint8Array([1, 2, 3]));
    t.sendDatagram(new Uint8Array([4, 5]));
    expect(t.datagramsSent).toHaveLength(2);
    expect(t.datagramsSent[0]).toEqual(new Uint8Array([1, 2, 3]));
  });

  it("dispatches simulated datagrams to onDatagram", () => {
    const t = new MockTransport();
    const received: Uint8Array[] = [];
    t.onDatagram = (data) => received.push(data);

    t.simulateDatagram(new Uint8Array([10, 20]));
    expect(received).toHaveLength(1);
    expect(received[0]).toEqual(new Uint8Array([10, 20]));
  });

  it("fires onClose on simulateClose", () => {
    const t = new MockTransport();
    const onClose = vi.fn();
    t.onClose = onClose;
    t.connected = true;

    t.simulateClose(1006, "gone");
    expect(onClose).toHaveBeenCalledWith(1006, "gone");
    expect(t.connected).toBe(false);
  });

  it("opens streams that can send and recv", async () => {
    const t = new MockTransport();
    const stream = await t.openStream();
    expect(stream).toBeDefined();

    const mockStream = t.lastStream()!;
    mockStream.enqueueRecv(new Uint8Array([42]));

    const data = await stream.recv();
    expect(data).toEqual(new Uint8Array([42]));

    await stream.send(new Uint8Array([99]));
    expect(mockStream.sent).toHaveLength(1);
    expect(mockStream.sent[0]).toEqual(new Uint8Array([99]));
  });
});

describe("MockStream", () => {
  it("queues recv data in order", async () => {
    const stream = new MockStream();
    stream.enqueueRecv(new Uint8Array([1]));
    stream.enqueueRecv(new Uint8Array([2]));

    expect(await stream.recv()).toEqual(new Uint8Array([1]));
    expect(await stream.recv()).toEqual(new Uint8Array([2]));
  });

  it("throws when recv queue is empty", async () => {
    const stream = new MockStream();
    await expect(stream.recv()).rejects.toThrow("recv queue empty");
  });

  it("tracks sent data", async () => {
    const stream = new MockStream();
    await stream.send(new Uint8Array([10]));
    await stream.send(new Uint8Array([20]));
    expect(stream.sent).toHaveLength(2);
  });
});
