import { Socket } from "phoenix";
import type { Channel } from "phoenix";

export type ConnectionStatus = "disconnected" | "connecting" | "connected" | "error";

export interface ConnectionState {
  socket: Socket;
  channel: Channel | null;
  status: ConnectionStatus;
}

const DEV_TOKEN = "qk_rw_dev_devdevdevdevdevdevdevdevdevdevde";

export function createConnection(
  onStatus: (status: ConnectionStatus, detail?: string) => void
): ConnectionState {
  const socket = new Socket("/ws", { params: { token: DEV_TOKEN } });

  socket.onOpen(() => onStatus("connecting"));
  socket.onClose(() => onStatus("disconnected"));
  socket.onError(() => onStatus("error", "WebSocket error"));

  const state: ConnectionState = { socket, channel: null, status: "disconnected" };

  socket.connect();
  onStatus("connecting");

  return state;
}

export function joinChannel(
  conn: ConnectionState,
  topic: string,
  onStatus: (status: ConnectionStatus, detail?: string) => void
): Channel {
  const channel = conn.socket.channel(topic, {});

  channel
    .join()
    .receive("ok", () => {
      conn.status = "connected";
      onStatus("connected", `Joined ${topic}`);
    })
    .receive("error", (resp) => {
      conn.status = "error";
      onStatus("error", `Join failed: ${JSON.stringify(resp)}`);
    });

  conn.channel = channel;
  return channel;
}
