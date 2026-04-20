export function renderTabBar(
  container: HTMLElement,
  openFiles: Array<{ id: string; name: string }>,
  activeId: string | null,
  onSelect: (id: string) => void,
  onClose: (id: string) => void
): void {
  container.innerHTML = "";

  for (const file of openFiles) {
    const tab = document.createElement("div");
    tab.className = `tab${file.id === activeId ? " active" : ""}`;

    const label = document.createElement("span");
    label.className = "tab-label";
    label.textContent = file.name;
    label.addEventListener("click", () => onSelect(file.id));

    tab.appendChild(label);

    if (openFiles.length > 1) {
      const closeBtn = document.createElement("button");
      closeBtn.className = "tab-close";
      closeBtn.textContent = "\u00d7";
      closeBtn.addEventListener("click", (e) => {
        e.stopPropagation();
        onClose(file.id);
      });
      tab.appendChild(closeBtn);
    }

    container.appendChild(tab);
  }
}
