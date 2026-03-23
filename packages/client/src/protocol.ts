// Wire formats match quanta-realtime-server/src/{auth,sync}.rs.

import type { QuantaDecoder, AuthResponseData } from "@quanta/delta-decoder";
import type { Stream } from "./transport.js";

export type { AuthResponseData };

const MAX_MESSAGE_BYTES = 16 * 1024 * 1024; // 16 MiB
const MAX_SCHEMA_BYTES = 1_048_576; // 1 MiB
const MAX_ENTITY_COUNT = 10_000;

export const FLAG_INCLUDES_SCHEMA = 0x01;

export interface EntityPayload {
  entitySlot: number;
  state: Uint8Array;
}

export interface InitialStateMessage {
  baselineTick: bigint;
  flags: number;
  schemaVersion: number;
  compiledSchema: Uint8Array | null;
  entities: EntityPayload[];
}

/**
 * Decode an InitialStateMessage from the binary wire format.
 *
 * ```
 * [baseline_tick:u64 BE][flags:u8][schema_version:u8]
 * [optional: schema_len:u32 + schema_bytes]
 * [entity_count:u32 BE]
 * [repeated: entity_slot:u32 + state_len:u32 + state_bytes]
 * ```
 */
export function decodeInitialState(bytes: Uint8Array): InitialStateMessage {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  // Minimum: baseline_tick(8) + flags(1) + schema_version(1) + entity_count(4)
  const MIN_SIZE = 14;

  if (bytes.length < MIN_SIZE) {
    throw new Error(
      `InitialStateMessage truncated: expected at least ${MIN_SIZE} bytes, got ${bytes.length}`,
    );
  }

  let pos = 0;

  const baselineTick = view.getBigUint64(pos);
  pos += 8;

  const flags = bytes[pos++];
  const schemaVersion = bytes[pos++];

  let compiledSchema: Uint8Array | null = null;
  if (flags & FLAG_INCLUDES_SCHEMA) {
    if (pos + 4 > bytes.length) {
      throw new Error("InitialStateMessage truncated: missing schema length");
    }
    const schemaLen = view.getUint32(pos);
    pos += 4;
    if (schemaLen > MAX_SCHEMA_BYTES) {
      throw new Error(`schema too large: ${schemaLen} bytes, max ${MAX_SCHEMA_BYTES}`);
    }
    if (pos + schemaLen > bytes.length) {
      throw new Error("InitialStateMessage truncated: missing schema bytes");
    }
    compiledSchema = bytes.slice(pos, pos + schemaLen);
    pos += schemaLen;
  }

  if (pos + 4 > bytes.length) {
    throw new Error("InitialStateMessage truncated: missing entity count");
  }
  const entityCount = view.getUint32(pos);
  pos += 4;

  if (entityCount > MAX_ENTITY_COUNT) {
    throw new Error(`entity count ${entityCount} exceeds max ${MAX_ENTITY_COUNT}`);
  }

  const entities: EntityPayload[] = [];
  for (let i = 0; i < entityCount; i++) {
    if (pos + 8 > bytes.length) {
      throw new Error(`InitialStateMessage truncated at entity ${i}`);
    }
    const entitySlot = view.getUint32(pos);
    pos += 4;
    const stateLen = view.getUint32(pos);
    pos += 4;
    if (pos + stateLen > bytes.length) {
      throw new Error(`InitialStateMessage truncated: entity ${i} state`);
    }
    const state = bytes.slice(pos, pos + stateLen);
    pos += stateLen;
    entities.push({ entitySlot, state });
  }

  return { baselineTick, flags, schemaVersion, compiledSchema, entities };
}

/** Assumes recv() delivers complete messages, not arbitrary byte fragments. */
export async function readLengthPrefixed(stream: Stream): Promise<Uint8Array> {
  const lenBuf = await stream.recv();

  if (lenBuf.length < 4) {
    throw new Error("stream: length prefix truncated");
  }
  const len = new DataView(
    lenBuf.buffer,
    lenBuf.byteOffset,
    lenBuf.byteLength,
  ).getUint32(0);

  if (len > MAX_MESSAGE_BYTES) {
    throw new Error(`message too large: ${len} bytes, max ${MAX_MESSAGE_BYTES}`);
  }

  if (lenBuf.length >= 4 + len) {
    return lenBuf.slice(4, 4 + len);
  }

  const payload = await stream.recv();
  if (payload.length < len) {
    throw new Error(
      `stream: payload truncated: expected ${len} bytes, got ${payload.length}`,
    );
  }
  return payload.slice(0, len);
}

export async function writeLengthPrefixed(
  stream: Stream,
  payload: Uint8Array,
): Promise<void> {
  const buf = new Uint8Array(4 + payload.length);
  new DataView(buf.buffer).setUint32(0, payload.length);
  buf.set(payload, 4);
  await stream.send(buf);
}

export async function performAuth(
  stream: Stream,
  decoder: QuantaDecoder,
  token: string,
  clientVersion: string,
  sessionToken?: bigint,
): Promise<AuthResponseData> {
  const authBytes = decoder.encodeAuthRequest(
    token,
    clientVersion,
    sessionToken ?? null,
  );
  await stream.send(authBytes);

  const respBytes = await stream.recv();
  const resp = decoder.decodeAuthResponse(respBytes);

  if (!resp.accepted) {
    throw new Error(`auth rejected: ${resp.reason}`);
  }

  return resp;
}

export async function performSync(
  stream: Stream,
  decoder: QuantaDecoder,
): Promise<InitialStateMessage> {
  const msgBytes = await readLengthPrefixed(stream);
  const msg = decodeInitialState(msgBytes);

  const ackBytes = decoder.encodeBaselineAck(msg.baselineTick);
  await stream.send(ackBytes);

  return msg;
}
