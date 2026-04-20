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

export function createSocket(
  onStatus: (status: ConnectionStatus, detail?: string) => void
): Socket {
  const socket = new Socket("/ws", { params: { token: DEV_TOKEN } });

  socket.onOpen(() => onStatus("connecting"));
  socket.onClose(() => onStatus("disconnected"));
  socket.onError(() => onStatus("error", "WebSocket error"));

  socket.connect();
  onStatus("connecting");

  return socket;
}

export function joinChannel(
  socket: Socket,
  topic: string,
  doc: LoroDoc,
  onStatus: (status: ConnectionStatus, detail?: string) => void,
  onJoin?: () => void
): { channel: Channel; cleanup: () => void } {
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
      onJoin?.();
    })
    .receive("error", (resp: unknown) => {
      onStatus("error", `Join failed: ${JSON.stringify(resp)}`);
    });

  return { channel, cleanup: () => channel.leave() };
}
