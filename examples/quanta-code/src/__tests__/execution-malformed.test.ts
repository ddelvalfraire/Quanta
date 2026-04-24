/**
 * H-4: JSON.parse without try/catch in execution.ts
 *
 * execution.ts line 47:
 *   const data = JSON.parse(payload.data as string) as Record<string, string>;
 *
 * There is no try/catch around this call. When `payload.data` is malformed
 * JSON the exception propagates into TypedEmitter.emit(), which swallows it
 * silently (emitter.ts lines 21-23). The output log receives no entry and the
 * user sees nothing.
 *
 * These tests FAIL today because:
 *   - No try/catch exists in the execution_output handler.
 *   - Malformed JSON throws synchronously inside the channel callback.
 *   - The emitter swallows the exception.
 *   - No error entry is appended to the output element.
 *
 * The tests will pass once a try/catch is added that calls appendOutput with
 * an error-class entry when JSON.parse throws.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";

// ---------------------------------------------------------------------------
// Minimal DOM stubs — jsdom is not configured; we build lightweight fakes.
// ---------------------------------------------------------------------------

type FakeChild = { className: string; textContent: string };

function makeFakeElement() {
  const children: FakeChild[] = [];
  return {
    children,
    scrollTop: 0,
    scrollHeight: 100,
    innerHTML: "",
    appendChild(child: FakeChild) {
      children.push(child);
    },
  };
}

// ---------------------------------------------------------------------------
// Minimal Channel stub — captures the handler registered via channel.on()
// so we can invoke it directly without Phoenix wiring.
// ---------------------------------------------------------------------------

type ChannelHandler = (msg: unknown) => void;

function makeStubChannel() {
  const handlers = new Map<string, ChannelHandler>();

  return {
    on(event: string, handler: ChannelHandler): number {
      handlers.set(event, handler);
      return handlers.size; // fake ref number
    },
    off(_event: string, _ref: number): void {},
    push: vi.fn(),
    /** Test helper: fire a registered handler directly */
    fire(event: string, msg: unknown): void {
      const h = handlers.get(event);
      if (!h) throw new Error(`No handler registered for "${event}"`);
      h(msg);
    },
  };
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("execution.ts — H-4: malformed JSON in execution_output handler", () => {
  let outputEl: ReturnType<typeof makeFakeElement>;
  let channel: ReturnType<typeof makeStubChannel>;
  let runBtn: { addEventListener: ReturnType<typeof vi.fn>; removeEventListener: ReturnType<typeof vi.fn> };
  let clearBtn: { addEventListener: ReturnType<typeof vi.fn>; removeEventListener: ReturnType<typeof vi.fn> };

  beforeEach(() => {
    outputEl = makeFakeElement();

    // Stub document.createElement so appendOutput can build child nodes
    vi.stubGlobal("document", {
      createElement(_tag: string): FakeChild {
        return { className: "", textContent: "" };
      },
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    });

    channel = makeStubChannel();
    runBtn = { addEventListener: vi.fn(), removeEventListener: vi.fn() };
    clearBtn = { addEventListener: vi.fn(), removeEventListener: vi.fn() };
  });

  it("FAILS today: malformed JSON in payload.data causes silent throw — no error entry in output log", async () => {
    const { setupExecution } = await import("../execution.js");

    setupExecution(
      channel as never,
      () => "",
      outputEl as never,
      runBtn as never,
      clearBtn as never,
    );

    // Fire the execution_output event with deliberately malformed JSON.
    // JSON.parse("{not valid") throws SyntaxError. There is no try/catch in
    // the handler, so the exception escapes the callback — proving the bug.
    // We catch it here just to keep the test running so we can also assert on
    // the output log state.
    try {
      channel.fire("execution_output", { data: "{not valid" });
    } catch {
      // expected — the throw escaping the handler IS the bug (no try/catch)
    }

    // FAILS today: no error entry is appended because the exception escaped
    // before appendOutput could be called. Once fixed (try/catch added), an
    // error entry will be present and this assertion will pass.
    const errorEntries = outputEl.children.filter(
      (c) =>
        c.className.includes("output-error") ||
        c.className.includes("parse-error"),
    );
    expect(errorEntries.length).toBeGreaterThan(0);
  });

  it("FAILS today: truncated JSON string leaves the output log empty", async () => {
    const { setupExecution } = await import("../execution.js");

    setupExecution(
      channel as never,
      () => "",
      outputEl as never,
      runBtn as never,
      clearBtn as never,
    );

    // Truncated object — JSON.parse throws SyntaxError. No try/catch means
    // the exception escapes; we catch it here to keep the test running.
    try {
      channel.fire("execution_output", {
        data: '{"status":"ok","stdout":"hello',
      });
    } catch {
      // expected — escaping exception proves the missing try/catch
    }

    // FAILS today: output log is empty because the SyntaxError escaped before
    // appendOutput could be called with an error entry.
    expect(outputEl.children.length).toBeGreaterThan(0);
  });

  it("FAILS today: empty string data field produces no parse-error entry", async () => {
    const { setupExecution } = await import("../execution.js");

    setupExecution(
      channel as never,
      () => "",
      outputEl as never,
      runBtn as never,
      clearBtn as never,
    );

    // JSON.parse("") also throws SyntaxError — same missing-try/catch path.
    try {
      channel.fire("execution_output", { data: "" });
    } catch {
      // expected — escaping exception proves the missing try/catch
    }

    // FAILS today: no error entry appended
    const errorEntries = outputEl.children.filter(
      (c) =>
        c.className.includes("output-error") ||
        c.className.includes("parse-error"),
    );
    expect(errorEntries.length).toBeGreaterThan(0);
  });
});
