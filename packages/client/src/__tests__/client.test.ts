import { describe, it, expect, vi } from "vitest";
import { ActorChannel } from "../actor-channel.js";
import { SchemaCache } from "../schema-cache.js";
import { DebugLogger } from "../debug.js";
import type { InitialStateMessage } from "../protocol.js";
import { createMockDecoder, MockSchemaHandle } from "./helpers.js";

/**
 * Since QuantaClient.connect() calls loadDecoder() which requires actual WASM,
 * we test the client logic by directly constructing the internal state and
 * exercising the methods. The QuantaClient class is a thin orchestrator over
 * Connection, SchemaCache, and ActorChannel — each tested independently.
 */

// Simulate the client's internal state after a successful connect()
function createClientState() {
  const decoder = createMockDecoder();
  const schemas = new SchemaCache();
  const debug = new DebugLogger({ enabled: false });
  const channels = new Map<number, ActorChannel>();

  const initialState: InitialStateMessage = {
    baselineTick: 100n,
    flags: 0x01,
    schemaVersion: 1,
    compiledSchema: new Uint8Array([0x01, 0x02]),
    entities: [
      { entitySlot: 0, state: new Uint8Array([10, 20]) },
      { entitySlot: 5, state: new Uint8Array([30, 40]) },
      { entitySlot: 12, state: new Uint8Array([50, 60]) },
    ],
  };

  // Create schema from compiled schema
  const schemaKey = "game:default";
  schemas.getOrCreate(schemaKey, initialState.compiledSchema!, decoder);

  const mockConn = {
    sendDatagram: vi.fn(),
    disconnect: vi.fn(),
    getTransportKind: () => "websocket" as const,
    getSessionId: () => 1,
  };

  return { decoder, schemas, debug, channels, initialState, mockConn, schemaKey };
}

function joinEntity(
  ctx: ReturnType<typeof createClientState>,
  entitySlot: number,
  actorType = "entity",
  actorId?: string,
): ActorChannel {
  const existing = ctx.channels.get(entitySlot);
  if (existing) return existing;

  const entity = ctx.initialState.entities.find(e => e.entitySlot === entitySlot);
  if (!entity) throw new Error(`Entity slot ${entitySlot} not found`);

  const schema = ctx.schemas.restore(ctx.schemaKey, ctx.decoder);
  if (!schema) throw new Error("No schema");

  const ch = new ActorChannel(
    entitySlot,
    "game",
    actorType,
    actorId ?? String(entitySlot),
    ctx.decoder,
    schema,
    ctx.initialState.schemaVersion,
    entity.state,
    ctx.mockConn as any,
  );
  ctx.channels.set(entitySlot, ch);
  return ch;
}

function handleDatagram(
  ctx: ReturnType<typeof createClientState>,
  data: Uint8Array,
) {
  if (data.length < 4) return;
  const view = new DataView(data.buffer, data.byteOffset, data.byteLength);
  const entitySlot = view.getUint32(0);
  const delta = data.subarray(4);
  const channel = ctx.channels.get(entitySlot);
  if (channel) {
    channel.applyDelta(delta);
  }
}

function handleReconnect(
  ctx: ReturnType<typeof createClientState>,
  msg: InitialStateMessage,
) {
  ctx.initialState = msg;
  if (msg.compiledSchema) {
    ctx.schemas.getOrCreate(ctx.schemaKey, msg.compiledSchema, ctx.decoder);
  }
  for (const entity of msg.entities) {
    const channel = ctx.channels.get(entity.entitySlot);
    if (channel) {
      channel.handleSnapshot(entity.state, msg.schemaVersion, 0);
    }
  }
}

describe("Client integration", () => {
  it("join creates ActorChannel for entity slot", () => {
    const ctx = createClientState();
    const ch = joinEntity(ctx, 0);
    expect(ch.entitySlot).toBe(0);
    expect(ch.getState()).toEqual({ x: 10, y: 20 });
  });

  it("join returns same channel for same slot", () => {
    const ctx = createClientState();
    const ch1 = joinEntity(ctx, 0);
    const ch2 = joinEntity(ctx, 0);
    expect(ch1).toBe(ch2);
  });

  it("join throws for unknown entity slot", () => {
    const ctx = createClientState();
    expect(() => joinEntity(ctx, 999)).toThrow("not found");
  });

  it("join sets actorType and actorId", () => {
    const ctx = createClientState();
    const ch = joinEntity(ctx, 5, "npc", "goblin-1");
    expect(ch.actorType).toBe("npc");
    expect(ch.actorId).toBe("goblin-1");
    expect(ch.namespace).toBe("game");
  });

  it("datagram routes delta to correct channel", () => {
    const ctx = createClientState();
    const ch = joinEntity(ctx, 0);
    const onDelta = vi.fn();
    ch.on("delta", onDelta);

    const datagram = new Uint8Array(4 + 2);
    new DataView(datagram.buffer).setUint32(0, 0); // entity_slot = 0
    datagram[4] = 0x01;
    datagram[5] = 0x00;

    handleDatagram(ctx, datagram);
    expect(onDelta).toHaveBeenCalledOnce();
  });

  it("datagram ignores unknown entity slots", () => {
    const ctx = createClientState();
    joinEntity(ctx, 0);

    const datagram = new Uint8Array(4 + 1);
    new DataView(datagram.buffer).setUint32(0, 99);
    datagram[4] = 0xff;

    // Should not throw
    handleDatagram(ctx, datagram);
  });

  it("reconnect updates existing channels with snapshots", () => {
    const ctx = createClientState();
    const ch = joinEntity(ctx, 0);
    const onFullState = vi.fn();
    ch.on("fullState", onFullState);

    const newMsg: InitialStateMessage = {
      baselineTick: 200n,
      flags: 0x01,
      schemaVersion: 2,
      compiledSchema: new Uint8Array([0x03]),
      entities: [
        { entitySlot: 0, state: new Uint8Array([99, 88]) },
      ],
    };

    handleReconnect(ctx, newMsg);

    expect(onFullState).toHaveBeenCalledOnce();
    expect(ch.getState()).toEqual({ x: 99, y: 88 });
    expect(ch.getSchemaVersion()).toBe(2);
  });

  it("channel send frames with entity_slot and input_seq", () => {
    const ctx = createClientState();
    const ch = joinEntity(ctx, 5); // slot 5
    ch.send(new Uint8Array([1, 2, 3]));

    const frame = ctx.mockConn.sendDatagram.mock.calls[0][0] as Uint8Array;
    const view = new DataView(frame.buffer, frame.byteOffset, frame.byteLength);
    expect(view.getUint32(0)).toBe(5); // entity_slot
    expect(view.getUint32(4)).toBe(0); // input_seq
    expect(frame.subarray(8)).toEqual(new Uint8Array([1, 2, 3]));
  });

  it("multiple entities can be joined independently", () => {
    const ctx = createClientState();
    const ch0 = joinEntity(ctx, 0);
    const ch5 = joinEntity(ctx, 5);
    const ch12 = joinEntity(ctx, 12);

    expect(ch0.getState()).toEqual({ x: 10, y: 20 });
    expect(ch5.getState()).toEqual({ x: 30, y: 40 });
    expect(ch12.getState()).toEqual({ x: 50, y: 60 });
  });
});
