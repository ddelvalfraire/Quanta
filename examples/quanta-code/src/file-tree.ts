import type { FileEntry } from "./types";

export function renderFileTree(
  container: HTMLElement,
  files: FileEntry[],
  activeId: string | null,
  onSelect: (id: string, name: string) => void,
  onCreate: (name: string) => void,
  onDelete: (id: string) => void
): void {
  container.innerHTML = "";

  const heading = document.createElement("div");
  heading.className = "sidebar-heading";
  heading.textContent = "Files";
  container.appendChild(heading);

  const list = document.createElement("div");
  list.className = "file-tree";

  if (files.length === 0) {
    const empty = document.createElement("div");
    empty.className = "file-tree-empty";
    empty.textContent = "No files yet";
    list.appendChild(empty);
  }

  for (const file of files) {
    const item = document.createElement("div");
    item.className = `file-item${file.id === activeId ? " active" : ""}`;

    const nameSpan = document.createElement("span");
    nameSpan.className = "file-item-name";
    nameSpan.textContent = file.name;
    nameSpan.addEventListener("click", () => onSelect(file.id, file.name));

    const actions = document.createElement("span");
    actions.className = "file-item-actions";

    const deleteBtn = document.createElement("button");
    deleteBtn.className = "file-item-delete";
    deleteBtn.textContent = "\u00d7";
    deleteBtn.title = "Delete file";
    deleteBtn.addEventListener("click", (e) => {
      e.stopPropagation();
      onDelete(file.id);
    });
    actions.appendChild(deleteBtn);

    item.append(nameSpan, actions);
    list.appendChild(item);
  }

  container.appendChild(list);

  const newBtn = document.createElement("button");
  newBtn.className = "btn-new-file";
  newBtn.textContent = "+ New File";
  newBtn.addEventListener("click", () => {
    const raw = prompt("File name:");
    if (!raw) return;
    const name = raw.trim().replace(/[/\\<>:"|?*]/g, "").slice(0, 64);
    if (name) {
      onCreate(name);
    }
  });
  container.appendChild(newBtn);
}
