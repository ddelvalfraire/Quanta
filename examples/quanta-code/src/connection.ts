import { Socket } from "phoenix";
import type { Channel } from "phoenix";
import { base64ToUint8 } from "./sync";
import type { LoroDoc } from "loro-crdt";

export type ConnectionStatus =
  | "disconnected"
  | "connecting"
  | "connected"
  | "error";

const DEV_TOKEN = "qk_rw_dev_devdevdevdevdevdevdevdevdevdevde";

export function connectAndJoin(
  topic: string,
  doc: LoroDoc,
  onStatus: (status: ConnectionStatus, detail?: string) => void
): { socket: Socket; channel: Channel } {
  const socket = new Socket("/ws", { params: { token: DEV_TOKEN } });

  socket.onOpen(() => onStatus("connecting"));
  socket.onClose(() => onStatus("disconnected"));
  socket.onError(() => onStatus("error", "WebSocket error"));

  socket.connect();
  onStatus("connecting");

  const channel = socket.channel(topic, {});

  channel
    .join()
    .receive("ok", (resp: unknown) => {
      const { snapshot } = resp as { snapshot: string };
      if (snapshot) {
        const bytes = base64ToUint8(snapshot);
        doc.import(bytes);
      }
      onStatus("connected", `Joined ${topic}`);
    })
    .receive("error", (resp: unknown) => {
      onStatus("error", `Join failed: ${JSON.stringify(resp)}`);
    });

  return { socket, channel };
}
