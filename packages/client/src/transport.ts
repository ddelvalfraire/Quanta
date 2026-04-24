export interface Stream {
  send(data: Uint8Array): Promise<void>;
  recv(): Promise<Uint8Array>;
  close(): void;
}

export interface Transport {
  sendDatagram(data: Uint8Array): void;
  onDatagram: ((data: Uint8Array) => void) | null;

  /** Client-initiated bidirectional stream (for auth). */
  openStream(): Promise<Stream>;
  /** Server-initiated bidirectional stream (for sync). */
  acceptStream(): Promise<Stream>;

  close(): void;
  onClose: ((code: number, reason: string) => void) | null;
  onError: ((err: unknown) => void) | null;
  readonly connected: boolean;
}


export interface WebTransportOptions {
  /** SHA-256 certificate hashes for self-signed dev certs. */
  serverCertificateHashes?: Array<{ algorithm: string; value: ArrayBuffer }>;
}

export class WebTransportAdapter implements Transport {
  private wt: WebTransport | null = null;
  private datagramWriter: WritableStreamDefaultWriter<Uint8Array> | null = null;
  private datagramReader: ReadableStreamDefaultReader<Uint8Array> | null = null;
  private closed = false;

  onDatagram: ((data: Uint8Array) => void) | null = null;
  onClose: ((code: number, reason: string) => void) | null = null;
  onError: ((err: unknown) => void) | null = null;

  get connected(): boolean {
    return this.wt !== null && !this.closed;
  }

  async connect(url: string, opts?: WebTransportOptions): Promise<void> {
    const wtOpts: WebTransportOptions & Record<string, unknown> = {};
    if (opts?.serverCertificateHashes) {
      wtOpts.serverCertificateHashes = opts.serverCertificateHashes;
    }

    const wt = new WebTransport(url, wtOpts);
    this.wt = wt;
    this.closed = false;
    await wt.ready;

    // If close() was called while we were waiting for `ready`, honour the
    // pending cancel: acquire the reader only to cancel it so the datagram
    // lock is released, then bail out.
    if (this.closed || this.wt !== wt) {
      try {
        const reader = wt.datagrams.readable.getReader();
        reader.cancel().catch(() => {});
      } catch {
        // Reader may already be closed — ignore
      }
      throw new Error("WebTransport closed before ready");
    }

    this.readDatagrams();

    wt.closed
      .then(({ closeCode, reason }) => {
        this.closed = true;
        this.onClose?.(closeCode ?? 0, reason ?? "");
      })
      .catch((err) => {
        this.closed = true;
        this.onError?.(err);
        this.onClose?.(1006, "transport error");
      });
  }

  sendDatagram(data: Uint8Array): void {
    if (!this.wt || this.closed) return;
    if (!this.datagramWriter) {
      this.datagramWriter = this.wt.datagrams.writable.getWriter();
    }
    // Fire-and-forget — don't await (unreliable semantics)
    this.datagramWriter.write(data).catch(() => {
      // Datagram send failure is expected if transport is closing
    });
  }

  async openStream(): Promise<Stream> {
    if (!this.wt || this.closed) {
      throw new Error("WebTransport not connected");
    }
    const bidi = await this.wt.createBidirectionalStream();
    return wrapBidiStream(bidi);
  }

  async acceptStream(): Promise<Stream> {
    if (!this.wt || this.closed) {
      throw new Error("WebTransport not connected");
    }
    const reader = this.wt.incomingBidirectionalStreams.getReader();
    const { value: bidi, done } = await reader.read();
    reader.releaseLock();
    if (done || !bidi) throw new Error("no incoming stream");
    return wrapBidiStream(bidi);
  }

  close(): void {
    this.closed = true;
    // Cancel the datagram reader so its lock is released even when the
    // transport is otherwise quiet. If close() is called before the reader
    // has been acquired (pre-`wt.ready`), `this.closed` is already set and
    // `readDatagrams` will cancel immediately on acquisition.
    this.datagramReader?.cancel().catch(() => {});
    this.datagramReader = null;
    this.datagramWriter?.close().catch(() => {});
    this.datagramWriter = null;
    this.wt?.close();
    this.wt = null;
  }

  private async readDatagrams(): Promise<void> {
    if (!this.wt) return;
    const reader = this.wt.datagrams.readable.getReader();
    this.datagramReader = reader;
    // If close() fired before the reader was acquired, honour the pending
    // cancel right away so the lock is released.
    if (this.closed) {
      reader.cancel().catch(() => {});
      this.datagramReader = null;
      return;
    }
    try {
      while (!this.closed) {
        const { value, done } = await reader.read();
        if (done) break;
        if (value) this.onDatagram?.(value);
      }
    } catch {
      // Reader will throw when transport closes — expected
    } finally {
      if (this.datagramReader === reader) {
        this.datagramReader = null;
      }
    }
  }
}

function wrapBidiStream(bidi: WebTransportBidirectionalStream): Stream {
  const writer = bidi.writable.getWriter();
  const reader = bidi.readable.getReader();
  return {
    async send(data: Uint8Array): Promise<void> {
      await writer.write(data);
    },
    async recv(): Promise<Uint8Array> {
      const { value, done } = await reader.read();
      if (done || !value) throw new Error("stream closed");
      return value;
    },
    close(): void {
      writer.close().catch(() => {});
      reader.cancel().catch(() => {});
    },
  };
}


// Frame flags — matches ws_session.rs
const WS_FLAG_RELIABLE = 0x00;
const WS_FLAG_UNRELIABLE = 0x01;

export class WebSocketAdapter implements Transport {
  private ws: WebSocket | null = null;
  private closed = false;
  private WebSocketCtor: typeof WebSocket;

  /**
   * Pending stream recv resolvers. WebSocket is single-connection, so
   * reliable messages are multiplexed on the same connection. We use a
   * simple queue to pair reliable messages with recv() calls.
   */
  private reliableQueue: Uint8Array[] = [];
  private reliableWaiters: Array<{
    resolve: (data: Uint8Array) => void;
    reject: (err: Error) => void;
  }> = [];

  onDatagram: ((data: Uint8Array) => void) | null = null;
  onClose: ((code: number, reason: string) => void) | null = null;
  onError: ((err: unknown) => void) | null = null;

  constructor(WebSocketCtor?: typeof WebSocket) {
    this.WebSocketCtor = WebSocketCtor ?? globalThis.WebSocket;
  }

  get connected(): boolean {
    return this.ws !== null && this.ws.readyState === this.WebSocketCtor.OPEN;
  }

  async connect(url: string): Promise<void> {
    return new Promise<void>((resolve, reject) => {
      this.ws = new this.WebSocketCtor(url);
      this.ws.binaryType = "arraybuffer";
      this.closed = false;

      this.ws.onopen = () => resolve();

      this.ws.onerror = (evt) => {
        if (!this.connected) {
          reject(new Error("WebSocket connection failed"));
        } else {
          this.onError?.(evt);
        }
      };

      this.ws.onclose = (evt) => {
        this.closed = true;
        const err = new Error("WebSocket closed");
        for (const { reject } of this.reliableWaiters) {
          reject(err);
        }
        this.reliableWaiters = [];
        this.onClose?.(evt.code, evt.reason);
      };

      this.ws.onmessage = (evt) => {
        if (!(evt.data instanceof ArrayBuffer)) return;
        const frame = new Uint8Array(evt.data);
        if (frame.length < 1) return;

        const flags = frame[0];
        const payload = frame.subarray(1);

        if (flags === WS_FLAG_UNRELIABLE) {
          this.onDatagram?.(payload);
        } else if (flags === WS_FLAG_RELIABLE) {
          const waiter = this.reliableWaiters.shift();
          if (waiter) {
            waiter.resolve(payload);
          } else {
            this.reliableQueue.push(payload);
          }
        }
      };
    });
  }

  sendDatagram(data: Uint8Array): void {
    if (!this.ws || this.closed) return;
    const frame = new Uint8Array(1 + data.length);
    frame[0] = WS_FLAG_UNRELIABLE;
    frame.set(data, 1);
    this.ws.send(frame);
  }

  async openStream(): Promise<Stream> {
    if (!this.ws || this.closed) {
      throw new Error("WebSocket not connected");
    }

    const self = this;

    return {
      async send(data: Uint8Array): Promise<void> {
        if (!self.ws || self.closed) throw new Error("WebSocket closed");
        const frame = new Uint8Array(1 + data.length);
        frame[0] = WS_FLAG_RELIABLE;
        frame.set(data, 1);
        self.ws.send(frame);
      },
      async recv(): Promise<Uint8Array> {
        const queued = self.reliableQueue.shift();
        if (queued) return queued;

        return new Promise<Uint8Array>((resolve, reject) => {
          if (self.closed) {
            reject(new Error("WebSocket closed"));
            return;
          }
          self.reliableWaiters.push({ resolve, reject });
        });
      },
      close(): void {
        // WebSocket streams share the connection — nothing to close per-stream
      },
    };
  }

  async acceptStream(): Promise<Stream> {
    // WebSocket multiplexes all reliable traffic on one connection,
    // so accepting a "server-initiated stream" is the same as opening one.
    return this.openStream();
  }

  close(): void {
    this.closed = true;
    this.ws?.close(1000, "client disconnect");
    this.ws = null;
  }
}
