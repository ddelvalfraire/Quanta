import type { EphemeralStore } from "loro-crdt";
import type { Channel } from "phoenix";
import { uint8ToBase64, base64ToUint8 } from "./sync";

export function setupEphemeralSync(
  ephemeral: EphemeralStore,
  channel: Channel
): () => void {
  const unsubLocal = ephemeral.subscribeLocalUpdates((bytes: Uint8Array) => {
    if (bytes.byteLength > 0) {
      channel.push("ephemeral_sync", {
        data: uint8ToBase64(bytes),
      });
    }
  });

  const refUpdate = channel.on("ephemeral_update", (msg: unknown) => {
    const payload = msg as Record<string, unknown>;
    if (typeof payload.data !== "string") return;
    ephemeral.apply(base64ToUint8(payload.data));
  });

  const refState = channel.on("ephemeral_state", (msg: unknown) => {
    const payload = msg as Record<string, unknown>;
    if (typeof payload.data !== "string") return;
    ephemeral.apply(base64ToUint8(payload.data));
  });

  return () => {
    unsubLocal();
    channel.off("ephemeral_update", refUpdate);
    channel.off("ephemeral_state", refState);
  };
}
