// Bootstrap: load wasm decoder, fetch server info, connect, authenticate,
// then drive the read/input/render loops.

import init from "../wasm-decoder/quanta_wasm_decoder";
import { startInputLoop } from "./input";
import { startRenderLoop } from "./render";
import { WorldState } from "./state";
import { authenticate, connect, hexToBytes, loadServerInfo } from "./transport";

// Static dev fixture — matches `DEFAULT_DEV_TOKEN` in particle-server.rs.
// TODO: replace with env-injected token before any non-local deployment.
const DEV_TOKEN = "qk_rw_dev_devdevdevdevdevdevdevdevdevdevde";

const status = document.getElementById("status") as HTMLDivElement;
const canvas = document.getElementById("canvas") as HTMLCanvasElement;

function setStatus(text: string) {
  status.textContent = text;
}

async function main(): Promise<void> {
  await init();

  const info = await loadServerInfo();
  setStatus(`loading schema v${info.schemaVersion}…`);

  const world = new WorldState(hexToBytes(info.schemaBytesHex));

  setStatus(`connecting to ${info.quicAddr}…`);
  const wt = await connect(info);

  const auth = await authenticate(wt, DEV_TOKEN);
  if (!auth.accepted) {
    throw new Error(`auth rejected: ${auth.reason || "(no reason)"}`);
  }

  let selfSlot: number | null = null;
  const refreshStatus = () => {
    setStatus(
      `connected · session ${auth.sessionId} · entities ${world.size()}`,
    );
  };
  refreshStatus();

  // Datagram reader. Treat the first entity we observe as "self" — an
  // approximation that works because the server's first fanout send for a
  // new client is its own entity (FULL_STATE).
  const reader = wt.datagrams.readable.getReader();
  (async () => {
    while (true) {
      const { value, done } = await reader.read();
      if (done) break;
      if (!value) continue;
      const res = world.ingestDatagram(value);
      if (res && selfSlot === null) selfSlot = res.slot;
      refreshStatus();
    }
  })().catch((e) => {
    setStatus(`disconnected: ${e?.message ?? e}`);
  });

  startInputLoop(wt);
  startRenderLoop(canvas, world, () => selfSlot);
}

main().catch((e) => {
  console.error(e);
  setStatus(`fatal: ${e?.message ?? e}`);
});
