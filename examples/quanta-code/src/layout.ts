export interface LayoutElements {
  sidebar: HTMLElement;
  tabBarContainer: HTMLElement;
  editorContainer: HTMLElement;
  outputPanel: HTMLElement;
  statusBar: HTMLElement;
  runBtn: HTMLElement;
  clearBtn: HTMLElement;
}

export function createLayout(root: HTMLElement): LayoutElements {
  root.innerHTML = "";

  const header = el("header", "header");
  header.innerHTML = `<span class="logo">Quanta Code</span><div class="header-right"><span class="user-count"></span><span class="status" id="conn-status">disconnected</span></div>`;

  const sidebar = el("aside", "sidebar");

  const tabBarContainer = el("div", "tab-bar");

  const editorContainer = el("div", "editor-container");

  const outputPanel = el("div", "output-panel");

  const panelHeading = el("div", "panel-heading");
  panelHeading.innerHTML = `<span>Output</span>`;

  const panelActions = el("div", "panel-actions");
  const runBtn = el("button", "btn-run");
  runBtn.textContent = "Run";
  runBtn.title = "Ctrl+Enter";
  const clearBtn = el("button", "btn-clear");
  clearBtn.textContent = "Clear";
  panelActions.append(runBtn, clearBtn);
  panelHeading.appendChild(panelActions);

  const outputLog = el("pre", "output-log");
  outputLog.id = "output-log";

  outputPanel.append(panelHeading, outputLog);

  const main = el("main", "main-area");
  main.append(tabBarContainer, editorContainer, outputPanel);

  const workspace = el("div", "workspace");
  workspace.append(sidebar, main);

  const statusBar = el("footer", "status-bar");
  statusBar.textContent = "Ready";

  root.append(header, workspace, statusBar);
  return { sidebar, tabBarContainer, editorContainer, outputPanel, statusBar, runBtn, clearBtn };
}

function el(tag: string, className: string): HTMLElement {
  const e = document.createElement(tag);
  e.className = className;
  return e;
}
