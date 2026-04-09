export interface LayoutElements {
  sidebar: HTMLElement;
  editorContainer: HTMLElement;
  outputPanel: HTMLElement;
  statusBar: HTMLElement;
}

export function createLayout(root: HTMLElement): LayoutElements {
  root.innerHTML = "";

  const header = el("header", "header");
  header.innerHTML = `<span class="logo">Quanta Code</span><span class="status" id="conn-status">disconnected</span>`;

  const sidebar = el("aside", "sidebar");
  sidebar.innerHTML = `<div class="sidebar-heading">Files</div><div class="sidebar-placeholder">Phase 5</div>`;

  const editorContainer = el("div", "editor-container");

  const outputPanel = el("div", "output-panel");
  outputPanel.innerHTML = `<div class="panel-heading">Output</div><pre class="output-log" id="output-log"></pre>`;

  const main = el("main", "main-area");
  main.append(editorContainer, outputPanel);

  const workspace = el("div", "workspace");
  workspace.append(sidebar, main);

  const statusBar = el("footer", "status-bar");
  statusBar.textContent = "Ready";

  root.append(header, workspace, statusBar);
  return { sidebar, editorContainer, outputPanel, statusBar };
}

function el(tag: string, className: string): HTMLElement {
  const e = document.createElement(tag);
  e.className = className;
  return e;
}
