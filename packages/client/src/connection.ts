import type { QuantaDecoder } from "@quanta/delta-decoder";
import type { Transport } from "./transport.js";
import { WebTransportAdapter, WebSocketAdapter } from "./transport.js";
import type { WebTransportOptions } from "./transport.js";
import { performAuth, performSync } from "./protocol.js";
import type { InitialStateMessage } from "./protocol.js";
import { TypedEmitter } from "./emitter.js";


export interface ConnectionOptions {
  url: string;
  apiKey: string;
  clientVersion?: string;
  serverCertificateHashes?: WebTransportOptions["serverCertificateHashes"];
  wsUrl?: string;
  WebSocketCtor?: typeof WebSocket;
  forceWebSocket?: boolean;
  connectTimeoutMs?: number;
}

export type TransportKind = "webtransport" | "websocket";

export type ConnectionState =
  | "disconnected"
  | "connecting"
  | "connected"
  | "reconnecting";

export interface ConnectionEvents {
  connected: (msg: InitialStateMessage, transport: TransportKind) => void;
  disconnected: (code: number, reason: string) => void;
  reconnecting: (attempt: number, delayMs: number) => void;
  datagram: (data: Uint8Array) => void;
  error: (err: unknown) => void;
}


const MAX_RECONNECT_DELAY_MS = 30_000;
const BASE_DELAY_MS = 1_000;
const JITTER_MS = 1_000;

function reconnectDelay(attempt: number): number {
  const exponential = Math.min(
    BASE_DELAY_MS * Math.pow(2, attempt),
    MAX_RECONNECT_DELAY_MS,
  );
  return exponential + Math.random() * JITTER_MS;
}


export class Connection extends TypedEmitter<ConnectionEvents> {
  private opts: Required<
    Pick<ConnectionOptions, "url" | "apiKey" | "clientVersion" | "connectTimeoutMs">
  > &
    ConnectionOptions;
  private decoder: QuantaDecoder;
  private transport: Transport | null = null;
  private transportKind: TransportKind = "webtransport";
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private reconnectAttempt = 0;
  private sessionId: bigint | null = null;
  private intentionalClose = false;

  state: ConnectionState = "disconnected";

  constructor(opts: ConnectionOptions, decoder: QuantaDecoder) {
    super();
    this.opts = {
      clientVersion: "0.1.0",
      connectTimeoutMs: 5_000,
      ...opts,
    };
    this.decoder = decoder;
  }

  async connect(): Promise<InitialStateMessage> {
    this.intentionalClose = false;
    this.state = "connecting";

    let transport: Transport;
    let transportKind: TransportKind;

    if (!this.opts.forceWebSocket && typeof globalThis.WebTransport !== "undefined") {
      try {
        const wt = new WebTransportAdapter();
        await withTimeout(
          wt.connect(this.opts.url, {
            serverCertificateHashes: this.opts.serverCertificateHashes,
          }),
          this.opts.connectTimeoutMs,
        );
        transport = wt;
        transportKind = "webtransport";
      } catch {
        transport = await this.connectWebSocket();
        transportKind = "websocket";
      }
    } else {
      transport = await this.connectWebSocket();
      transportKind = "websocket";
    }

    this.transport = transport;
    this.transportKind = transportKind;

    transport.onDatagram = (data) => this.emit("datagram", data);
    transport.onClose = (code, reason) => this.handleTransportClose(code, reason);
    transport.onError = (err) => this.emit("error", err);

    const stream = await transport.openStream();
    try {
      const authResp = await performAuth(
        stream,
        this.decoder,
        this.opts.apiKey,
        this.opts.clientVersion,
        this.sessionId ?? undefined,
      );
      this.sessionId = authResp.sessionId as bigint;

      const syncStream = await transport.acceptStream();
      const initialState = await performSync(syncStream, this.decoder);
      syncStream.close();

      this.state = "connected";
      this.reconnectAttempt = 0;
      this.emit("connected", initialState, transportKind);

      return initialState;
    } finally {
      stream.close();
    }
  }

  sendDatagram(data: Uint8Array): void {
    this.transport?.sendDatagram(data);
  }

  disconnect(): void {
    this.intentionalClose = true;
    this.clearReconnectTimer();
    this.transport?.close();
    this.transport = null;
    this.state = "disconnected";
    this.emit("disconnected", 1000, "client disconnect");
  }

  getSessionId(): bigint | null {
    return this.sessionId;
  }

  getTransportKind(): TransportKind {
    return this.transportKind;
  }

  private async connectWebSocket(): Promise<Transport> {
    const wsUrl = this.opts.wsUrl ?? deriveWsUrl(this.opts.url);
    const ws = new WebSocketAdapter(this.opts.WebSocketCtor);
    await withTimeout(ws.connect(wsUrl), this.opts.connectTimeoutMs);
    return ws;
  }

  private handleTransportClose(code: number, reason: string): void {
    this.transport = null;

    if (this.intentionalClose) {
      this.state = "disconnected";
      return;
    }

    this.state = "reconnecting";
    this.emit("disconnected", code, reason);
    this.scheduleReconnect();
  }

  private scheduleReconnect(): void {
    this.clearReconnectTimer();
    const delay = reconnectDelay(this.reconnectAttempt);
    this.emit("reconnecting", this.reconnectAttempt, delay);

    this.reconnectTimer = setTimeout(async () => {
      this.reconnectAttempt++;
      try {
        await this.connect();
      } catch {
        if (!this.intentionalClose) {
          this.scheduleReconnect();
        }
      }
    }, delay);
  }

  private clearReconnectTimer(): void {
    if (this.reconnectTimer !== null) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
  }
}

function deriveWsUrl(url: string): string {
  return url.replace(/^https:\/\//, "wss://").replace(/^http:\/\//, "ws://");
}

function withTimeout<T>(promise: Promise<T>, ms: number): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(
      () => reject(new Error(`timeout after ${ms}ms`)),
      ms,
    );
    promise.then(
      (val) => {
        clearTimeout(timer);
        resolve(val);
      },
      (err) => {
        clearTimeout(timer);
        reject(err);
      },
    );
  });
}
