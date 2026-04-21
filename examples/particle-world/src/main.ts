// Bootstrap: load wasm decoder, fetch server info, connect, authenticate,
// then drive the read/input/render loops.

import init from "../wasm-decoder/quanta_wasm_decoder";
import { startHud } from "./hud";
import { startInputLoop } from "./input";
import { SelfPredictor } from "./predictor";
import { startRenderLoop } from "./render";
import { FLAG_WELCOME, WorldState } from "./state";
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
  const predictor = new SelfPredictor();
  const refreshStatus = () => {
    setStatus(
      `connected · session ${auth.sessionId} · entities ${world.size()}`,
    );
  };
  refreshStatus();

  // Datagram reader. The server sends a FLAG_WELCOME datagram right after
  // RegisterClient, telling us which slot is ours — that's the only
  // reliable signal, because the fanout's priority accumulator routinely
  // sends fast-moving NPCs ahead of the idle self entity. Every later
  // snapshot for the self slot is fed to the predictor for reconciliation.
  const reader = wt.datagrams.readable.getReader();
  (async () => {
    while (true) {
      const { value, done } = await reader.read();
      if (done) break;
      if (!value) continue;
      const res = world.ingestDatagram(value);
      if (!res) continue;
      if ((res.flags & FLAG_WELCOME) !== 0) {
        selfSlot = res.slot;
      } else if (selfSlot !== null && res.slot === selfSlot) {
        const snap = world.latestSnapshot(selfSlot);
        if (snap) {
          predictor.reconcile(
            snap.posX,
            snap.posZ,
            snap.velX,
            snap.velZ,
            snap.lastInputSeq,
          );
        }
      }
      refreshStatus();
    }
  })().catch((e) => {
    setStatus(`disconnected: ${e?.message ?? e}`);
  });

  // Debug handle for in-browser smoothness measurement.
  (window as unknown as { __quanta?: unknown }).__quanta = {
    world,
    predictor,
    getSelfSlot: () => selfSlot,
  };

  startInputLoop(wt, predictor);
  startHud(world, predictor, () => selfSlot);
  await startRenderLoop(canvas, world, predictor, () => selfSlot);
}

main().catch((e) => {
  console.error(e);
  setStatus(`fatal: ${e?.message ?? e}`);
});
