import type { DecodedState, SchemaHandle } from "@quanta/delta-decoder";

/** Connection options for QuantaClient. */
export interface ClientOptions {
  /** WebSocket URL, e.g. "wss://example.com/socket/websocket" */
  url: string;
  /** Auth token sent as a socket param. */
  token: string;
}

/** Options for joining an actor channel. */
export interface JoinOptions {
  /** QSCH schema bytes. Required until server-side schema delivery is implemented. */
  schemaBytes: Uint8Array;
  /** Optional params sent with the join message. */
  params?: Record<string, unknown>;
}

/** Events emitted by an ActorChannel. */
export interface ActorChannelEvents {
  /** Fired when a delta is applied. Receives the decoded state, changed field names, and sequence number. */
  delta: (state: DecodedState, changedFields: string[], seq: number) => void;
  /** Fired when the server signals node draining. */
  draining: (reconnectMs: number) => void;
  /** Fired when the actor stops on the server. */
  stopped: () => void;
  /** Fired on channel error. */
  error: (err: Error) => void;
}

export type EventName = keyof ActorChannelEvents;

/** Re-export for convenience. */
export type { DecodedState, SchemaHandle };
