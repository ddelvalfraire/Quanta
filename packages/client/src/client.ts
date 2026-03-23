import { loadDecoder } from "@quanta/delta-decoder";
import type { QuantaDecoder } from "@quanta/delta-decoder";
import { ActorChannel } from "./actor-channel.js";
import { SchemaCache } from "./schema-cache.js";
import { Connection } from "./connection.js";
import type { TransportKind } from "./connection.js";
import type { InitialStateMessage, EntityPayload } from "./protocol.js";
import type { ClientOptions } from "./types.js";
import { DebugLogger } from "./debug.js";

export class QuantaClient {
  readonly schemas: SchemaCache;

  private opts: ClientOptions;
  private decoder: QuantaDecoder | null = null;
  private connection: Connection | null = null;
  private channels = new Map<number, ActorChannel>();
  private initialState: InitialStateMessage | null = null;
  private cachedSchemaVersion: number | null = null;
  private debug: DebugLogger;

  constructor(opts: ClientOptions) {
    this.opts = opts;
    this.schemas = new SchemaCache();
    this.debug = new DebugLogger(opts.debug);
  }

  async connect(): Promise<void> {
    this.decoder = await loadDecoder();

    const conn = new Connection(
      {
        url: this.opts.url,
        apiKey: this.opts.apiKey,
        serverCertificateHashes: this.opts.serverCertificateHashes,
        wsUrl: this.opts.wsUrl,
        WebSocketCtor: this.opts.WebSocketCtor,
        forceWebSocket: this.opts.forceWebSocket,
      },
      this.decoder,
    );

    conn.on("datagram", (data) => this.handleDatagram(data));
    conn.on("connected", (msg, transport) => this.handleReconnect(msg, transport));

    this.connection = conn;

    const msg = await conn.connect();
    this.initialState = msg;

    this.applySchema(msg);

    this.debug.log("normal", `connected via ${conn.getTransportKind()}, entities=${msg.entities.length}`);
  }

  join(entitySlot: number, actorType = "entity", actorId?: string): ActorChannel {
    if (!this.decoder) {
      throw new Error("QuantaClient.connect() must be called before join()");
    }

    const existing = this.channels.get(entitySlot);
    if (existing) return existing;

    const entity = this.initialState?.entities.find(
      (e) => e.entitySlot === entitySlot,
    );
    if (!entity) {
      throw new Error(`Entity slot ${entitySlot} not found in initial state`);
    }

    const schema = this.schemas.restore(this.schemaKey(), this.decoder);
    if (!schema) {
      throw new Error("No schema available — initial state did not include compiled schema");
    }

    const channel = new ActorChannel(
      entitySlot,
      this.opts.namespace,
      actorType,
      actorId ?? String(entitySlot),
      this.decoder,
      schema,
      this.initialState?.schemaVersion ?? 0,
      entity.state,
      this.connection,
    );

    this.channels.set(entitySlot, channel);
    this.debug.log("normal", `joined entity slot=${entitySlot}`);
    return channel;
  }

  getEntities(): EntityPayload[] {
    return this.initialState?.entities ?? [];
  }

  getChannel(entitySlot: number): ActorChannel | undefined {
    return this.channels.get(entitySlot);
  }

  getTransportKind(): TransportKind | null {
    return this.connection?.getTransportKind() ?? null;
  }

  getDebug(): DebugLogger {
    return this.debug;
  }

  disconnect(): void {
    for (const channel of this.channels.values()) {
      channel.disconnect();
    }
    this.channels.clear();
    this.schemas.clear();
    this.connection?.disconnect();
    this.connection = null;
    this.decoder = null;
    this.initialState = null;
    this.debug.log("normal", "disconnected");
  }

  private applySchema(msg: InitialStateMessage): void {
    if (msg.compiledSchema && this.decoder) {
      this.schemas.getOrCreate(this.schemaKey(), msg.compiledSchema, this.decoder);
      this.cachedSchemaVersion = msg.schemaVersion;
    } else if (
      this.cachedSchemaVersion !== null &&
      msg.schemaVersion !== this.cachedSchemaVersion
    ) {
      throw new Error(
        `schema version changed (${this.cachedSchemaVersion} → ${msg.schemaVersion}) but server did not include compiled schema`,
      );
    }
  }

  private schemaKey(): string {
    return `${this.opts.namespace}:default`;
  }

  /**
   * Route incoming datagram to the matching entity channel.
   * Datagram format: [entity_slot:u32 BE][delta bytes]
   */
  private handleDatagram(data: Uint8Array): void {
    if (data.length < 4) return;

    const view = new DataView(data.buffer, data.byteOffset, data.byteLength);
    const entitySlot = view.getUint32(0);
    const delta = data.subarray(4);

    const channel = this.channels.get(entitySlot);
    if (channel) {
      const startTime = performance.now();
      channel.applyDelta(delta);
      const elapsed = performance.now() - startTime;
      this.debug.recordDelta(entitySlot, delta.length, elapsed, channel.getState());
    }
  }

  private handleReconnect(msg: InitialStateMessage, transport: TransportKind): void {
    this.initialState = msg;
    this.applySchema(msg);

    for (const entity of msg.entities) {
      const channel = this.channels.get(entity.entitySlot);
      if (channel) {
        channel.handleSnapshot(entity.state, msg.schemaVersion, 0);
      }
    }

    this.debug.log("normal", `reconnected via ${transport}, entities=${msg.entities.length}`);
  }
}
