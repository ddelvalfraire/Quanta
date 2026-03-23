import { describe, it, expect } from "vitest";
import {
  decodeInitialState,
  FLAG_INCLUDES_SCHEMA,
  performAuth,
  performSync,
  type AuthResponseData,
} from "../protocol.js";
import {
  MockStream,
  createMockDecoder,
  buildMockAuthResponse,
  buildInitialStateMessage,
  lengthPrefix,
} from "./helpers.js";

describe("decodeInitialState", () => {
  it("decodes message with no schema", () => {
    const bytes = buildInitialStateMessage({
      baselineTick: 100n,
      schemaVersion: 3,
      entities: [
        { entitySlot: 0, state: new Uint8Array([1, 2, 3]) },
        { entitySlot: 5, state: new Uint8Array([4, 5]) },
      ],
    });

    const msg = decodeInitialState(bytes);
    expect(msg.baselineTick).toBe(100n);
    expect(msg.flags).toBe(0);
    expect(msg.schemaVersion).toBe(3);
    expect(msg.compiledSchema).toBeNull();
    expect(msg.entities).toHaveLength(2);
    expect(msg.entities[0].entitySlot).toBe(0);
    expect(msg.entities[0].state).toEqual(new Uint8Array([1, 2, 3]));
    expect(msg.entities[1].entitySlot).toBe(5);
    expect(msg.entities[1].state).toEqual(new Uint8Array([4, 5]));
  });

  it("decodes message with compiled schema", () => {
    const schema = new Uint8Array([0xde, 0xad, 0xbe, 0xef]);
    const bytes = buildInitialStateMessage({
      baselineTick: 42n,
      schemaVersion: 1,
      compiledSchema: schema,
      entities: [{ entitySlot: 0, state: new Uint8Array([10]) }],
    });

    const msg = decodeInitialState(bytes);
    expect(msg.flags & FLAG_INCLUDES_SCHEMA).toBe(FLAG_INCLUDES_SCHEMA);
    expect(msg.compiledSchema).toEqual(schema);
    expect(msg.entities).toHaveLength(1);
  });

  it("decodes empty entity list", () => {
    const bytes = buildInitialStateMessage({
      baselineTick: 0n,
      schemaVersion: 0,
      entities: [],
    });

    const msg = decodeInitialState(bytes);
    expect(msg.entities).toHaveLength(0);
  });

  it("throws on truncated header", () => {
    expect(() => decodeInitialState(new Uint8Array(5))).toThrow("truncated");
  });

  it("throws on truncated entity state", () => {
    const full = buildInitialStateMessage({
      baselineTick: 1n,
      schemaVersion: 1,
      entities: [{ entitySlot: 0, state: new Uint8Array([1, 2, 3, 4, 5]) }],
    });
    // Chop off the last 2 bytes of entity state
    const truncated = full.slice(0, full.length - 2);
    expect(() => decodeInitialState(truncated)).toThrow("truncated");
  });

  it("handles large entity counts", () => {
    const entities = Array.from({ length: 200 }, (_, i) => ({
      entitySlot: i,
      state: new Uint8Array([(i & 0xff)]),
    }));
    const bytes = buildInitialStateMessage({
      baselineTick: 9999n,
      schemaVersion: 2,
      entities,
    });

    const msg = decodeInitialState(bytes);
    expect(msg.entities).toHaveLength(200);
    expect(msg.entities[199].entitySlot).toBe(199);
  });
});

describe("performAuth", () => {
  it("sends auth request and decodes response", async () => {
    const decoder = createMockDecoder();
    const stream = new MockStream();

    // Queue the server's auth response for the stream recv
    const authResp = buildMockAuthResponse({
      sessionId: 42,
      accepted: true,
      reason: "",
    });
    stream.enqueueRecv(authResp);

    const resp = await performAuth(stream, decoder, "tok_test", "0.1.0");

    expect(resp.sessionId).toBe(42n);
    expect(resp.accepted).toBe(true);
    expect(stream.sent).toHaveLength(1); // auth request was sent
  });

  it("throws on rejected auth", async () => {
    const decoder = createMockDecoder();
    const stream = new MockStream();

    stream.enqueueRecv(
      buildMockAuthResponse({
        sessionId: 0,
        accepted: false,
        reason: "invalid_token",
      }),
    );

    await expect(
      performAuth(stream, decoder, "bad_token", "0.1.0"),
    ).rejects.toThrow("auth rejected: invalid_token");
  });

  it("passes session token for fast reconnect", async () => {
    const decoder = createMockDecoder();
    const stream = new MockStream();

    stream.enqueueRecv(
      buildMockAuthResponse({ sessionId: 99, accepted: true, reason: "" }),
    );

    await performAuth(stream, decoder, "tok", "0.1.0", 55);

    // Verify the auth request was sent (first and only sent message)
    const sentBytes = stream.sent[0];
    const view = new DataView(
      sentBytes.buffer,
      sentBytes.byteOffset,
      sentBytes.byteLength,
    );
    const len = view.getUint32(0);
    const json = JSON.parse(
      new TextDecoder().decode(sentBytes.slice(4, 4 + len)),
    );
    expect(json.sessionToken).toBe(55);
  });
});

describe("performSync", () => {
  it("receives InitialStateMessage and sends BaselineAck", async () => {
    const decoder = createMockDecoder();
    const stream = new MockStream();

    const stateMsg = buildInitialStateMessage({
      baselineTick: 500n,
      schemaVersion: 1,
      compiledSchema: new Uint8Array([0xaa, 0xbb]),
      entities: [{ entitySlot: 0, state: new Uint8Array([10, 20]) }],
    });

    // Queue the length-prefixed initial state message
    stream.enqueueRecv(lengthPrefix(stateMsg));

    const msg = await performSync(stream, decoder);

    expect(msg.baselineTick).toBe(500n);
    expect(msg.schemaVersion).toBe(1);
    expect(msg.compiledSchema).toEqual(new Uint8Array([0xaa, 0xbb]));
    expect(msg.entities).toHaveLength(1);

    // Verify baseline ack was sent
    expect(stream.sent).toHaveLength(1);
    const ackBytes = stream.sent[0];
    const view = new DataView(
      ackBytes.buffer,
      ackBytes.byteOffset,
      ackBytes.byteLength,
    );
    const ackLen = view.getUint32(0);
    const ackJson = JSON.parse(
      new TextDecoder().decode(ackBytes.slice(4, 4 + ackLen)),
    );
    expect(ackJson.baselineTick).toBe(500);
  });
});
