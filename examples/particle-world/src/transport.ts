// WebTransport lifecycle: fetch server info, open the QUIC session with
// the published cert hash, run the bitcode auth handshake on a bidi stream.

import {
  decode_auth_response,
  encode_auth_request,
} from "../wasm-decoder/quanta_wasm_decoder";

export type ServerInfo = {
  quicAddr: string;
  certSha256Hex: string;
  schemaVersion: number;
  schemaBytesHex: string;
};

export async function loadServerInfo(): Promise<ServerInfo> {
  const resp = await fetch("/server-info.json", { cache: "no-store" });
  if (!resp.ok) {
    throw new Error(
      `fetch /server-info.json failed: ${resp.status}. Is particle-server running?`,
    );
  }
  const raw = await resp.json();
  return {
    quicAddr: raw.quic_addr,
    certSha256Hex: raw.cert_sha256_hex,
    schemaVersion: raw.schema_version,
    schemaBytesHex: raw.schema_bytes_hex,
  };
}

export function hexToBytes(hex: string): Uint8Array<ArrayBuffer> {
  if (hex.length % 2 !== 0) throw new Error(`hex length odd: ${hex.length}`);
  const buf = new ArrayBuffer(hex.length / 2);
  const out = new Uint8Array(buf);
  for (let i = 0; i < out.length; i++) {
    const pair = hex.slice(i * 2, i * 2 + 2);
    const byte = parseInt(pair, 16);
    if (Number.isNaN(byte)) {
      throw new Error(`invalid hex at offset ${i * 2}: "${pair}"`);
    }
    out[i] = byte;
  }
  return out;
}

export async function connect(info: ServerInfo): Promise<WebTransport> {
  const url = `https://${info.quicAddr}/`;
  const wt = new WebTransport(url, {
    serverCertificateHashes: [
      { algorithm: "sha-256", value: hexToBytes(info.certSha256Hex) },
    ],
  });
  await wt.ready;
  return wt;
}

export type AuthResult = {
  sessionId: bigint;
  accepted: boolean;
  reason: string;
};

export async function authenticate(
  wt: WebTransport,
  token: string,
): Promise<AuthResult> {
  const stream = await wt.createBidirectionalStream();
  const writer = stream.writable.getWriter();
  const reader = stream.readable.getReader();

  const req = encode_auth_request(token, "0.1.0", undefined, undefined);
  await writer.write(req);

  let leftover: Uint8Array = new Uint8Array(0);
  const [lenBuf, after1] = await readExact(reader, 4, leftover);
  leftover = after1;
  const len = new DataView(
    lenBuf.buffer,
    lenBuf.byteOffset,
    lenBuf.byteLength,
  ).getUint32(0, false);
  const [payload, _after2] = await readExact(reader, len, leftover);

  const wire = new Uint8Array(4 + len);
  wire.set(lenBuf, 0);
  wire.set(payload, 4);
  const resp = decode_auth_response(wire) as AuthResult;

  try {
    await writer.close();
  } catch {
    /* server may already have closed the stream */
  }
  reader.releaseLock();
  return resp;
}

// Returns exactly `n` bytes plus any leftover bytes from the chunk that
// crossed the `n` boundary — those belong to the next read.
async function readExact(
  reader: ReadableStreamDefaultReader<Uint8Array>,
  n: number,
  prefix: Uint8Array,
): Promise<[Uint8Array, Uint8Array]> {
  const out = new Uint8Array(n);
  let got = 0;
  if (prefix.length > 0) {
    const take = Math.min(prefix.length, n);
    out.set(prefix.subarray(0, take), 0);
    got = take;
    if (prefix.length > n) {
      return [out, prefix.subarray(n)];
    }
  }
  while (got < n) {
    const { value, done } = await reader.read();
    if (done) throw new Error("stream ended before auth response");
    if (!value) continue;
    const take = Math.min(value.length, n - got);
    out.set(value.subarray(0, take), got);
    got += take;
    if (value.length > take) {
      return [out, value.subarray(take)];
    }
  }
  return [out, new Uint8Array(0)];
}
