import type { DecodedState, SchemaHandle } from "@quanta/delta-decoder";
import type { WebTransportOptions } from "./transport.js";

export interface ClientOptions {
  /** e.g., "https://example.com:4433" */
  url: string;
  apiKey: string;
  namespace: string;
  debug?: DebugOptions;
  serverCertificateHashes?: WebTransportOptions["serverCertificateHashes"];
  /** If not set, derived from url by replacing https→wss. */
  wsUrl?: string;
  /** For Node.js — pass the `ws` package's WebSocket constructor. */
  WebSocketCtor?: typeof WebSocket;
  forceWebSocket?: boolean;
}

export interface DebugOptions {
  enabled?: boolean;
  verbosity?: "minimal" | "normal" | "verbose";
}

export interface ActorChannelEvents {
  delta: (state: DecodedState, changedFields: string[], seq: number) => void;
  fullState: (state: DecodedState) => void;
  draining: (reconnectMs: number) => void;
  stopped: () => void;
  error: (err: Error) => void;
}

export type EventName = keyof ActorChannelEvents;

export type { DecodedState, SchemaHandle };
