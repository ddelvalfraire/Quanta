import { createLayout } from "./layout";
import { createEditor } from "./editor";
import { connectAndJoin } from "./connection";
import { setupSync } from "./sync";
import { setupEphemeralSync } from "./ephemeral-sync";
import { setupPresence } from "./presence";
import type { ConnectionStatus } from "./connection";

const COLORS = [
  { colorClassName: "user-blue" },
  { colorClassName: "user-red" },
  { colorClassName: "user-green" },
  { colorClassName: "user-yellow" },
  { colorClassName: "user-purple" },
];

function pickColor(index: number) {
  return COLORS[index % COLORS.length];
}

const app = document.getElementById("app")!;
const { editorContainer, statusBar } = createLayout(app);

const statusEl = document.getElementById("conn-status")!;
const outputLog = document.getElementById("output-log")!;

function log(msg: string) {
  const line = document.createElement("div");
  line.textContent = `[${new Date().toLocaleTimeString()}] ${msg}`;
  outputLog.appendChild(line);
  outputLog.scrollTop = outputLog.scrollHeight;
}

function updateStatus(status: ConnectionStatus, detail?: string) {
  statusEl.textContent = status;
  statusEl.className = `status status-${status}`;
  statusBar.textContent = detail ?? status;
  log(`Connection: ${status}${detail ? " — " + detail : ""}`);
}

const userIndex = Math.floor(Math.random() * 1000);
const userColor = pickColor(userIndex);
const userName = `User-${userIndex}`;

const { doc, ephemeral } = createEditor(editorContainer, {
  name: userName,
  ...userColor,
});

const { channel } = connectAndJoin("crdt:dev:file:demo", doc, updateStatus);

setupSync(doc, channel);
setupEphemeralSync(ephemeral, channel);

setupPresence(channel, (users) => {
  const countEl = document.querySelector(".user-count");
  if (countEl) {
    countEl.textContent = `${users.length} online`;
  }
});
