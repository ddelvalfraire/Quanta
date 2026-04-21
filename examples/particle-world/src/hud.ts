// On-screen debug HUD. Surfaces the same metrics we inspect from the
// Playwright eval — framerate, entity count, predictor vs server drift,
// seq-ack lag, pending inputs. Lets you verify at a glance that the
// prediction / interpolation pipeline is healthy.

import { SelfPredictor } from "./predictor";
import { WorldState } from "./state";

/** Frames sampled for rolling FPS / frame-time means. Short enough to
 *  reflect momentary hitches, long enough not to jitter each tick. */
const FRAME_WINDOW = 60;

/** Minimum milliseconds between HUD DOM updates. At 60 fps rAF the HUD
 *  would otherwise re-run `world.interpolate` (O(N) entities) and
 *  re-write innerHTML on every render frame — measurable extra work
 *  on the main thread. 66 ms (~15 Hz) is fast enough for readability
 *  without competing with the render loop for the frame budget. The
 *  frametime sampler still fires every rAF so the max-frame metric
 *  captures hitches precisely; only the DOM write is throttled. */
const HUD_UPDATE_INTERVAL_MS = 66;

export function startHud(
  world: WorldState,
  predictor: SelfPredictor,
  getSelfSlot: () => number | null,
): () => void {
  const el = document.getElementById("hud");
  if (!el) return () => {};

  const frameTimes: number[] = [];
  let lastT = performance.now();
  let lastHudWriteT = 0;
  let stopped = false;

  // Sampled-NPC motion tracking. We latch a single moving NPC and
  // record its frame-to-frame VELOCITY (position delta / frame dt),
  // NOT the raw position delta. Raw Δ/frame is proportional to wall-
  // clock frame time — a single 65 ms GC pause inflates that delta to
  // 4× normal even when the NPC's motion through time is perfectly
  // smooth. Dividing by dt normalizes out rAF variance, so CV near 0
  // means actual motion is smooth (regardless of render cadence) and
  // CV spikes mean the server / interpolation pipeline is producing
  // genuinely irregular positions. The HUD also reports the raw max
  // frame dt separately so the user can distinguish jitter-in-data
  // from jitter-in-browser.
  let sampledNpcSlot: number | null = null;
  let lastNpcX: number | null = null;
  let lastNpcZ: number | null = null;
  let lastNpcT: number | null = null;
  const NPC_VEL_WINDOW = 60; // 1 second at 60 fps
  const npcVelocities: number[] = [];

  const frame = () => {
    if (stopped) return;
    const now = performance.now();
    const dt = now - lastT;
    lastT = now;
    frameTimes.push(dt);
    if (frameTimes.length > FRAME_WINDOW) frameTimes.shift();

    // Throttle the DOM+interpolate work. Frame-time sampling above
    // runs every rAF (needed for accurate max/p99); everything below
    // only runs when HUD_UPDATE_INTERVAL_MS has elapsed.
    if (now - lastHudWriteT < HUD_UPDATE_INTERVAL_MS) {
      requestAnimationFrame(frame);
      return;
    }
    lastHudWriteT = now;

    const meanFrame = frameTimes.reduce((s, x) => s + x, 0) / frameTimes.length;
    const fps = meanFrame > 0 ? 1000 / meanFrame : 0;
    const p99 =
      frameTimes.length > 5
        ? [...frameTimes].sort((a, b) => a - b)[
            Math.floor(frameTimes.length * 0.99)
          ]
        : meanFrame;

    const selfSlot = getSelfSlot();
    const pred = predictor.current();
    const snap = selfSlot !== null ? world.latestSnapshot(selfSlot) : null;

    const pending = predictor.pendingCount();
    const latestPendingSeq = predictor.latestPendingSeq();
    const ackedSeq = snap ? snap.lastInputSeq : 0;
    const seqLag = latestPendingSeq > 0 ? latestPendingSeq - ackedSeq : 0;

    const serverPos = snap
      ? `${snap.posX.toFixed(1)},${snap.posZ.toFixed(1)}`
      : "—";
    const driftX = snap ? pred.posX - snap.posX : 0;
    const driftZ = snap ? pred.posZ - snap.posZ : 0;
    const drift = snap ? Math.hypot(driftX, driftZ) : 0;
    // Expected lead = pending inputs × one-tick position advance at
    // current speed. With canonical Gambetta replay, `drift` should
    // track this value closely — anything far above it indicates dropped
    // inputs or desync.
    const speed = Math.hypot(pred.velX, pred.velZ);
    const expectedLead = pending * speed * (1 / 30);

    // NPC motion sampling — measure VELOCITY (u/s), not raw Δ/frame.
    // Raw Δ/frame tracks rAF timing; u/s tracks actual motion smoothness.
    const remotes = world.interpolate(now, selfSlot);
    if (sampledNpcSlot === null) {
      for (const e of remotes) {
        if (Math.hypot(e.velX, e.velZ) > 50) {
          sampledNpcSlot = e.slot;
          lastNpcX = e.x;
          lastNpcZ = e.z;
          lastNpcT = now;
          break;
        }
      }
    }
    let npcVelMean = 0;
    let npcVelCv = 0;
    let npcVelMax = 0;
    let npcVelMin = 0;
    if (sampledNpcSlot !== null) {
      const e = remotes.find((r) => r.slot === sampledNpcSlot);
      if (e) {
        if (
          lastNpcX !== null &&
          lastNpcZ !== null &&
          lastNpcT !== null &&
          now > lastNpcT
        ) {
          const dtSec = (now - lastNpcT) / 1000;
          const posDelta = Math.hypot(e.x - lastNpcX, e.z - lastNpcZ);
          const vel = posDelta / dtSec;
          npcVelocities.push(vel);
          if (npcVelocities.length > NPC_VEL_WINDOW) npcVelocities.shift();
        }
        lastNpcX = e.x;
        lastNpcZ = e.z;
        lastNpcT = now;
      }
      if (npcVelocities.length > 5) {
        npcVelMean =
          npcVelocities.reduce((s, x) => s + x, 0) / npcVelocities.length;
        const std = Math.sqrt(
          npcVelocities.reduce((s, x) => s + (x - npcVelMean) ** 2, 0) /
            npcVelocities.length,
        );
        npcVelCv = npcVelMean > 1 ? std / npcVelMean : 0;
        npcVelMax = Math.max(...npcVelocities);
        npcVelMin = Math.min(...npcVelocities);
      }
    }

    // Max rAF frame time in the rolling window = the size of any
    // browser hitch (GC pause, main-thread stall). Independent of the
    // NPC motion metric — if this is high but NPC velocity CV is low,
    // the jitter is in the BROWSER not the simulation.
    const maxFrameMs = frameTimes.length > 0 ? Math.max(...frameTimes) : 0;

    const fpsClass = fps < 50 ? "warn" : "v";
    const hitchClass = maxFrameMs > 25 ? "warn" : "v";
    const lagClass = seqLag > 30 ? "warn" : "v";
    const anomalousDrift = drift > Math.max(expectedLead * 2 + 20, 60);
    const driftClass = anomalousDrift ? "warn" : "v";
    const jitterClass = npcVelCv > 0.3 ? "warn" : "v";

    el.innerHTML =
      `<span class="k">fps</span> <span class="${fpsClass}">${fps.toFixed(0)}</span>` +
      ` (p99 <span class="v">${p99.toFixed(1)}ms</span>)\n` +
      `<span class="k">entities</span> <span class="v">${world.size()}</span>\n` +
      `<span class="k">self slot</span> <span class="v">${selfSlot ?? "—"}</span>\n` +
      `<span class="k">pending</span> <span class="${lagClass}">${pending}</span> inputs\n` +
      `<span class="k">seq ack</span> <span class="v">${ackedSeq}</span> / sent <span class="v">${latestPendingSeq}</span>\n` +
      `<span class="k">seq lag</span> <span class="${lagClass}">${seqLag}</span>\n` +
      `<span class="k">pred pos</span> <span class="v">${pred.posX.toFixed(1)},${pred.posZ.toFixed(1)}</span>\n` +
      `<span class="k">server pos</span> <span class="v">${serverPos}</span>\n` +
      `<span class="k">pred lead</span> <span class="${driftClass}">${drift.toFixed(1)}</span> u (expected <span class="v">${expectedLead.toFixed(1)}</span>)\n` +
      `<span class="k">pred vel</span> <span class="v">${speed.toFixed(0)}</span> u/s\n` +
      `<span class="k">max frame</span> <span class="${hitchClass}">${maxFrameMs.toFixed(1)}ms</span>\n` +
      `<span class="k">npc slot</span> <span class="v">${sampledNpcSlot ?? "—"}</span>\n` +
      `<span class="k">npc vel</span> <span class="v">${npcVelMean.toFixed(0)}</span> u/s (min <span class="v">${npcVelMin.toFixed(0)}</span>, max <span class="v">${npcVelMax.toFixed(0)}</span>)\n` +
      `<span class="k">npc vel cv</span> <span class="${jitterClass}">${npcVelCv.toFixed(2)}</span>`;

    requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);

  return () => {
    stopped = true;
  };
}
