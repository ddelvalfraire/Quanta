import { loadDecoder } from "@quanta/delta-decoder";
import type { QuantaDecoder } from "@quanta/delta-decoder";
import { ActorChannel } from "./actor-channel.js";
import { SchemaCache } from "./schema-cache.js";
import type { ClientOptions, JoinOptions } from "./types.js";

/**
 * Top-level Quanta client.
 *
 * Manages WASM initialization, schema caching, and actor channel lifecycle.
 * Transport is intentionally left abstract — callers provide their own
 * Phoenix Channel connection and wire up events to ActorChannel methods.
 */
export class QuantaClient {
  readonly url: string;
  readonly token: string;
  readonly schemas: SchemaCache;

  private decoder: QuantaDecoder | null = null;
  private channels = new Map<string, ActorChannel>();

  constructor(opts: ClientOptions) {
    this.url = opts.url;
    this.token = opts.token;
    this.schemas = new SchemaCache();
  }

  /** Initialize the WASM decoder. Call once before joining channels. */
  async connect(): Promise<void> {
    this.decoder = await loadDecoder();
  }

  /**
   * Create an ActorChannel for the given actor.
   *
   * The caller is responsible for:
   * 1. Connecting to the Phoenix socket
   * 2. Joining the channel topic (`channel.topic`)
   * 3. Passing the base64-decoded join reply `state` as `initialState`
   * 4. Routing "state_update" pushes to `channel.applyDelta()`
   * 5. Routing "node_draining" pushes to `channel.handleDraining()`
   * 6. Routing "actor_stopped" pushes to `channel.handleStopped()`
   */
  joinActor(
    namespace: string,
    actorType: string,
    actorId: string,
    initialState: Uint8Array,
    opts: JoinOptions,
  ): ActorChannel {
    if (!this.decoder) {
      throw new Error("QuantaClient.connect() must be called before joinActor()");
    }

    const schemaKey = `${namespace}:${actorType}`;
    const schema = this.schemas.getOrCreate(schemaKey, opts.schemaBytes, this.decoder);

    const channel = new ActorChannel(
      namespace,
      actorType,
      actorId,
      this.decoder,
      schema,
      initialState,
    );

    this.channels.set(channel.topic, channel);
    return channel;
  }

  /** Get an existing actor channel by topic. */
  getChannel(topic: string): ActorChannel | undefined {
    return this.channels.get(topic);
  }

  /** Remove an actor channel. */
  removeChannel(topic: string): void {
    this.channels.delete(topic);
  }

  /** Disconnect: clear all channels and schema cache. */
  disconnect(): void {
    this.channels.clear();
    this.schemas.clear();
    this.decoder = null;
  }
}
