// 2D Canvas render loop. Projects world coords [-5000, 5000] on each
// axis into the canvas. Self entity drawn blue, others gray.

import { WorldState } from "./state";

const WORLD_SIZE = 10_000;

export function startRenderLoop(
  canvas: HTMLCanvasElement,
  world: WorldState,
  selfSlot: () => number | null,
): () => void {
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("canvas 2d context unavailable");

  let stopped = false;
  const tick = () => {
    if (stopped) return;
    const w = canvas.width;
    const h = canvas.height;
    const scale = Math.min(w, h) / WORLD_SIZE;
    ctx.fillStyle = "#000";
    ctx.fillRect(0, 0, w, h);
    const me = selfSlot();
    for (const { slot, fields } of world.entries()) {
      const x = w / 2 + (fields["pos-x"] ?? 0) * scale;
      const y = h / 2 + (fields["pos-z"] ?? 0) * scale;
      ctx.beginPath();
      ctx.arc(x, y, 6, 0, Math.PI * 2);
      ctx.fillStyle = slot === me ? "#6cf" : "#999";
      ctx.fill();
    }
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
  return () => {
    stopped = true;
  };
}
