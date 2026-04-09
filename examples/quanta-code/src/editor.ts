import { EditorView, basicSetup } from "codemirror";
import { javascript } from "@codemirror/lang-javascript";
import { EditorState } from "@codemirror/state";

const INITIAL_DOC = `// Welcome to Quanta Code
// Start typing — collaborative editing comes in Phase 3

function greet(name) {
  console.log("Hello, " + name + "!");
}

greet("Quanta");
`;

export function createEditor(parent: HTMLElement): EditorView {
  const state = EditorState.create({
    doc: INITIAL_DOC,
    extensions: [
      basicSetup,
      javascript(),
      EditorView.theme({
        "&": { height: "100%" },
        ".cm-scroller": { overflow: "auto" },
      }),
    ],
  });

  return new EditorView({ state, parent });
}
