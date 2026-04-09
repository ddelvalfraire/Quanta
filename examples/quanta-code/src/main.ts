import { createLayout } from "./layout";
import { createEditor } from "./editor";
import { createConnection, joinChannel } from "./connection";

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

createEditor(editorContainer);

const conn = createConnection((status, detail) => {
  statusEl.textContent = status;
  statusEl.className = `status status-${status}`;
  statusBar.textContent = detail ?? status;
  log(`Connection: ${status}${detail ? " — " + detail : ""}`);
});

joinChannel(conn, "crdt:default:file:demo", (status, detail) => {
  statusEl.textContent = status;
  statusEl.className = `status status-${status}`;
  if (detail) log(detail);
});
