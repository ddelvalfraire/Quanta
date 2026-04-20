import type { LoroDoc, EphemeralStore, UndoManager } from "loro-crdt";
import type { Channel } from "phoenix";
import type { EditorView } from "codemirror";

export interface FileEntry {
  id: string;
  name: string;
}

export interface OpenFile {
  id: string;
  name: string;
  doc: LoroDoc;
  ephemeral: EphemeralStore;
  undoManager: UndoManager;
  view: EditorView | null;
  channel: Channel;
  cleanups: Array<() => void>;
}
