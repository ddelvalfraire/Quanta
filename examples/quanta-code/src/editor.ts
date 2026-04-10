import { EditorView, basicSetup } from "codemirror";
import { javascript } from "@codemirror/lang-javascript";
import { EditorState } from "@codemirror/state";
import { LoroExtensions } from "loro-codemirror";
import { LoroDoc, EphemeralStore, UndoManager } from "loro-crdt";

export interface UserInfo {
  name: string;
  colorClassName: string;
}

export interface EditorContext {
  view: EditorView;
  doc: LoroDoc;
  ephemeral: EphemeralStore;
  undoManager: UndoManager;
}

export function createEditor(
  parent: HTMLElement,
  user: UserInfo
): EditorContext {
  const doc = new LoroDoc();
  const ephemeral = new EphemeralStore();
  const undoManager = new UndoManager(doc, {});

  const state = EditorState.create({
    extensions: [
      basicSetup,
      javascript(),
      LoroExtensions(
        doc,
        { user: { name: user.name, colorClassName: user.colorClassName }, ephemeral },
        undoManager
      ),
      EditorView.theme({
        "&": { height: "100%" },
        ".cm-scroller": { overflow: "auto" },
      }),
    ],
  });

  const view = new EditorView({ state, parent });
  return { view, doc, ephemeral, undoManager };
}
