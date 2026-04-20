import { createLayout } from "./layout";
import { createSocket } from "./connection";
import type { ConnectionStatus } from "./connection";
import { setupProject } from "./project";
import { renderFileTree } from "./file-tree";
import { renderTabBar } from "./tab-bar";
import { setupPresence } from "./presence";

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
const { sidebar, tabBarContainer, editorContainer, statusBar, runBtn, clearBtn } =
  createLayout(app);

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

const socket = createSocket(updateStatus);

const project = setupProject(
  socket,
  editorContainer,
  { name: userName, ...userColor },
  outputLog,
  runBtn,
  clearBtn,
  updateStatus,
  (files) => {
    renderFileTree(
      sidebar,
      files,
      project.getActiveFileId(),
      (id, name) => project.openFile(id, name),
      (name) => project.createFile(name),
      (id) => project.deleteFile(id)
    );
  },
  (openFiles, activeId) => {
    renderTabBar(
      tabBarContainer,
      openFiles,
      activeId,
      (id) => project.activateFile(id),
      (id) => project.closeFile(id)
    );
  },
  () => {
    const files = project.getFiles();
    if (files.length === 0) {
      project.createFile("main.js");
    } else if (!project.getActiveFileId()) {
      project.openFile(files[0].id, files[0].name);
    }
  }
);

setupPresence(project.projectChannel, (users) => {
  const countEl = document.querySelector(".user-count");
  if (countEl) {
    countEl.textContent = `${users.length} online`;
  }
});
