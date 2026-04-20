import { LoroDoc, EphemeralStore, UndoManager } from "loro-crdt";
import type { LoroTree, LoroTreeNode } from "loro-crdt";
import type { Socket, Channel } from "phoenix";
import { createEditor } from "./editor";
import { setupSync } from "./sync";
import { setupEphemeralSync } from "./ephemeral-sync";
import { setupExecution } from "./execution";
import { joinChannel } from "./connection";
import type { ConnectionStatus } from "./connection";
import type { FileEntry, OpenFile } from "./types";

export interface ProjectManager {
  projectChannel: Channel;
  openFile(id: string, name: string): void;
  closeFile(id: string): void;
  activateFile(id: string): void;
  createFile(name: string): void;
  renameFile(id: string, newName: string): void;
  deleteFile(id: string): void;
  getFiles(): FileEntry[];
  getOpenFiles(): Array<{ id: string; name: string }>;
  getActiveFileId(): string | null;
  destroy(): void;
}

export function setupProject(
  socket: Socket,
  editorContainer: HTMLElement,
  user: { name: string; colorClassName: string },
  outputLog: HTMLElement,
  runBtn: HTMLElement,
  clearBtn: HTMLElement,
  onStatus: (status: ConnectionStatus, detail?: string) => void,
  onFilesChange: (files: FileEntry[]) => void,
  onActiveChange: (
    openFiles: Array<{ id: string; name: string }>,
    activeId: string | null
  ) => void,
  onReady?: () => void
): ProjectManager {
  const projectDoc = new LoroDoc();
  const { channel: projectChannel, cleanup: cleanupProject } = joinChannel(
    socket,
    "crdt:dev:project:default",
    projectDoc,
    onStatus,
    onReady
  );

  setupSync(projectDoc, projectChannel);

  const tree: LoroTree = projectDoc.getTree("tree");
  const openFiles = new Map<string, OpenFile>();
  let activeFileId: string | null = null;
  let executionCleanup: (() => void) | null = null;

  function getFiles(): FileEntry[] {
    const nodes = tree.getNodes();
    return nodes.map((node: LoroTreeNode) => ({
      id: node.id as string,
      name: (node.data.get("name") as string) ?? "untitled",
    }));
  }

  function getOpenFiles(): Array<{ id: string; name: string }> {
    return Array.from(openFiles.values()).map((f) => ({
      id: f.id,
      name: f.name,
    }));
  }

  function getActiveFileId(): string | null {
    return activeFileId;
  }

  function notifyFilesChange() {
    onFilesChange(getFiles());
  }

  function notifyActiveChange() {
    onActiveChange(getOpenFiles(), activeFileId);
  }

  function openFile(id: string, name: string) {
    if (openFiles.has(id)) {
      activateFile(id);
      return;
    }

    const doc = new LoroDoc();
    const ephemeral = new EphemeralStore();
    const undoManager = new UndoManager(doc, {});

    const { channel, cleanup: channelCleanup } = joinChannel(
      socket,
      `crdt:dev:file:${id}`,
      doc,
      onStatus
    );

    const cleanups: Array<() => void> = [channelCleanup];
    cleanups.push(setupSync(doc, channel));
    cleanups.push(setupEphemeralSync(ephemeral, channel));

    const file: OpenFile = {
      id,
      name,
      doc,
      ephemeral,
      undoManager,
      view: null,
      channel,
      cleanups,
    };

    openFiles.set(id, file);
    activateFile(id);
  }

  function closeFile(id: string) {
    const file = openFiles.get(id);
    if (!file) return;

    if (file.view) {
      file.view.destroy();
    }
    for (const cleanup of file.cleanups) {
      cleanup();
    }
    openFiles.delete(id);

    if (activeFileId === id) {
      const remaining = Array.from(openFiles.keys());
      if (remaining.length > 0) {
        activateFile(remaining[remaining.length - 1]);
      } else {
        if (executionCleanup) {
          executionCleanup();
          executionCleanup = null;
        }
        activeFileId = null;
        editorContainer.innerHTML = "";
      }
    }

    notifyActiveChange();
  }

  function activateFile(id: string) {
    const file = openFiles.get(id);
    if (!file) return;

    // Destroy previous active view
    if (activeFileId && activeFileId !== id) {
      const prev = openFiles.get(activeFileId);
      if (prev?.view) {
        prev.view.destroy();
        prev.view = null;
      }
    }

    // Clean up previous execution wiring
    if (executionCleanup) {
      executionCleanup();
      executionCleanup = null;
    }

    activeFileId = id;

    // Create fresh EditorView for the active file
    editorContainer.innerHTML = "";
    const { view } = createEditor(editorContainer, {
      name: user.name,
      colorClassName: user.colorClassName,
      doc: file.doc,
      ephemeral: file.ephemeral,
      undoManager: file.undoManager,
    });
    file.view = view;

    // Wire execution to the active file's channel and view
    executionCleanup = setupExecution(
      file.channel,
      () => view.state.doc.toString(),
      outputLog,
      runBtn,
      clearBtn
    );

    notifyActiveChange();
  }

  function createFile(name: string) {
    const node = tree.createNode();
    node.data.set("name", name);
    projectDoc.commit();
    openFile(node.id as string, name);
  }

  function renameFile(id: string, newName: string) {
    const node = tree.getNodeByID(id as `${number}@${number}`);
    if (!node) return;
    node.data.set("name", newName);
    projectDoc.commit();

    const file = openFiles.get(id);
    if (file) {
      file.name = newName;
    }

    notifyFilesChange();
    notifyActiveChange();
  }

  function deleteFile(id: string) {
    closeFile(id);
    tree.delete(id as `${number}@${number}`);
    projectDoc.commit();
    notifyFilesChange();
  }

  // Subscribe to project doc changes to update file tree
  const unsubTree = projectDoc.subscribe(() => {
    notifyFilesChange();
    // Update names of open files if they changed
    for (const [fileId, file] of openFiles) {
      const node = tree.getNodeByID(fileId as `${number}@${number}`);
      if (node) {
        const name = (node.data.get("name") as string) ?? "untitled";
        if (file.name !== name) {
          file.name = name;
          notifyActiveChange();
        }
      }
    }
  });

  function destroy() {
    unsubTree();
    if (executionCleanup) {
      executionCleanup();
      executionCleanup = null;
    }
    for (const file of Array.from(openFiles.values())) {
      if (file.view) file.view.destroy();
      for (const cleanup of file.cleanups) cleanup();
    }
    openFiles.clear();
    activeFileId = null;
    cleanupProject();
  }

  return {
    projectChannel,
    openFile,
    closeFile,
    activateFile,
    createFile,
    renameFile,
    deleteFile,
    getFiles,
    getOpenFiles,
    getActiveFileId,
    destroy,
  };
}
