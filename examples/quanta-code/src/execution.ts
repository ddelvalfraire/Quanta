import type { Channel } from "phoenix";
import { uint8ToBase64 } from "./sync";

export function setupExecution(
  channel: Channel,
  getCode: () => string,
  outputEl: HTMLElement,
  runBtn: HTMLElement,
  clearBtn: HTMLElement
): () => void {
  function appendOutput(text: string, className: string) {
    const line = document.createElement("div");
    line.className = `output-entry ${className}`;
    line.textContent = text;
    outputEl.appendChild(line);
    outputEl.scrollTop = outputEl.scrollHeight;
  }

  function run() {
    const code = getCode();
    if (!code.trim()) return;

    const payload = JSON.stringify({ type: "run", code });
    const encoded = uint8ToBase64(new TextEncoder().encode(payload));
    channel.push("message", { payload: encoded });
  }

  function handleKeyboard(e: KeyboardEvent) {
    if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
      e.preventDefault();
      run();
    }
  }

  function clear() {
    outputEl.innerHTML = "";
  }

  runBtn.addEventListener("click", run);
  clearBtn.addEventListener("click", clear);
  document.addEventListener("keydown", handleKeyboard);

  const ref = channel.on("execution_output", (msg: unknown) => {
    const payload = msg as Record<string, unknown>;
    if (typeof payload.data !== "string") return;

    let data: Record<string, string>;
    try {
      data = JSON.parse(payload.data as string) as Record<string, string>;
    } catch (err) {
      const reason = err instanceof Error ? err.message : "unknown error";
      appendOutput(`parse error: ${reason}`, "output-error parse-error");
      return;
    }

    if (data.status === "ok") {
      if (data.stdout) {
        for (const line of data.stdout.split("\n")) {
          if (line) appendOutput(line, "output-stdout");
        }
      }
      if (data.stderr) {
        for (const line of data.stderr.split("\n")) {
          if (line) appendOutput(line, "output-stderr");
        }
      }
    } else if (data.status === "error") {
      appendOutput(data.error || "Unknown error", "output-error");
    }
  });

  return () => {
    runBtn.removeEventListener("click", run);
    clearBtn.removeEventListener("click", clear);
    document.removeEventListener("keydown", handleKeyboard);
    channel.off("execution_output", ref);
  };
}
