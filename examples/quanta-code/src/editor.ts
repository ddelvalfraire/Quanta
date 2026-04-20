import { EditorView, basicSetup } from "codemirror";
import { javascript } from "@codemirror/lang-javascript";
import { EditorState } from "@codemirror/state";
import { LoroExtensions } from "loro-codemirror";
import { LoroDoc, EphemeralStore, UndoManager } from "loro-crdt";

export interface EditorOptions {
  name: string;
  colorClassName: string;
  doc?: LoroDoc;
  ephemeral?: EphemeralStore;
  undoManager?: UndoManager;
}

export interface EditorContext {
  view: EditorView;
  doc: LoroDoc;
  ephemeral: EphemeralStore;
  undoManager: UndoManager;
}

export function createEditor(
  parent: HTMLElement,
  opts: EditorOptions
): EditorContext {
  const doc = opts.doc ?? new LoroDoc();
  const ephemeral = opts.ephemeral ?? new EphemeralStore();
  const undoManager = opts.undoManager ?? new UndoManager(doc, {});

  const state = EditorState.create({
    extensions: [
      basicSetup,
      javascript(),
      LoroExtensions(
        doc,
        { user: { name: opts.name, colorClassName: opts.colorClassName }, ephemeral },
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
