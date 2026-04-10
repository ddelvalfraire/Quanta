import type { LoroDoc } from "loro-crdt";
import type { Channel } from "phoenix";

export function setupSync(doc: LoroDoc, channel: Channel): () => void {
  let lastVersion = doc.version();

  const unsubscribe = doc.subscribe((event) => {
    if (event.by === "import") return;
    const update = doc.export({ mode: "update", from: lastVersion });
    lastVersion = doc.version();
    if (update.byteLength > 0) {
      channel.push("crdt_update", {
        delta: uint8ToBase64(update),
      });
    }
  });

  const ref = channel.on("crdt_update", (msg: unknown) => {
    const payload = msg as Record<string, unknown>;
    if (typeof payload.delta !== "string") return;
    const bytes = base64ToUint8(payload.delta);
    doc.import(bytes);
    lastVersion = doc.version();
  });

  return () => {
    unsubscribe();
    channel.off("crdt_update", ref);
  };
}

export function uint8ToBase64(bytes: Uint8Array): string {
  let binary = "";
  for (let i = 0; i < bytes.byteLength; i++) {
    binary += String.fromCharCode(bytes[i]);
  }
  return btoa(binary);
}

export function base64ToUint8(base64: string): Uint8Array {
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i) & 0xff;
  }
  return bytes;
}
