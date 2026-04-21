// PixiJS renderer: WebGL-batched sprites so the canvas holds thousands
// of entities at 60 fps, with client-side velocity extrapolation between
// 20 Hz authoritative snapshots so motion looks continuous.

import { Application, Container, Graphics, Sprite } from "pixi.js";

import { WorldState } from "./state";

const WORLD_SIZE = 10_000; // clamp [-5000, 5000] per axis

function hueFromSlot(slot: number): number {
  // Golden-angle hash → spread distinct hues across the swarm.
  const h = (slot * 137.508) % 360;
  return hslToRgb(h / 360, 0.65, 0.6);
}

function hslToRgb(h: number, s: number, l: number): number {
  const a = s * Math.min(l, 1 - l);
  const f = (n: number) => {
    const k = (n + h * 12) % 12;
    const c = l - a * Math.max(-1, Math.min(k - 3, 9 - k, 1));
    return Math.round(c * 255);
  };
  return (f(0) << 16) | (f(8) << 8) | f(4);
}

export async function startRenderLoop(
  canvas: HTMLCanvasElement,
  world: WorldState,
  selfSlot: () => number | null,
): Promise<() => void> {
  const app = new Application();
  await app.init({
    canvas,
    resizeTo: canvas.parentElement ?? window,
    background: 0x0a0c14,
    antialias: true,
    autoDensity: true,
    resolution: window.devicePixelRatio || 1,
  });

  // Static backdrop: faint radial vignette + grid so the black-on-black
  // demo doesn't look like a loading screen.
  const grid = new Graphics();
  app.stage.addChild(grid);
  const drawGrid = () => {
    const w = app.screen.width;
    const h = app.screen.height;
    grid.clear();
    // Vignette
    grid.rect(0, 0, w, h).fill({ color: 0x111827, alpha: 0.0 });
    // Axis cross at world origin
    const cx = w / 2;
    const cy = h / 2;
    grid
      .moveTo(0, cy)
      .lineTo(w, cy)
      .stroke({ width: 1, color: 0x1f2a3a, alpha: 0.6 });
    grid
      .moveTo(cx, 0)
      .lineTo(cx, h)
      .stroke({ width: 1, color: 0x1f2a3a, alpha: 0.6 });
    // Concentric rings every 1000 world units
    const scale = Math.min(w, h) / WORLD_SIZE;
    for (let r = 1000; r <= 5000; r += 1000) {
      grid
        .circle(cx, cy, r * scale)
        .stroke({ width: 1, color: 0x1f2a3a, alpha: 0.35 });
    }
  };
  drawGrid();
  window.addEventListener("resize", drawGrid);

  // One glowing-disc texture, reused across every sprite for cheap
  // WebGL batching.
  const disc = new Graphics();
  disc
    .circle(16, 16, 6)
    .fill({ color: 0xffffff, alpha: 1.0 });
  disc
    .circle(16, 16, 12)
    .fill({ color: 0xffffff, alpha: 0.18 });
  const texture = app.renderer.generateTexture({
    target: disc,
    resolution: 2,
  });

  const swarm = new Container();
  swarm.blendMode = "add";
  app.stage.addChild(swarm);

  const selfLayer = new Container();
  app.stage.addChild(selfLayer);

  const sprites = new Map<number, Sprite>();

  function spriteFor(slot: number): Sprite {
    let s = sprites.get(slot);
    if (!s) {
      s = new Sprite(texture);
      s.anchor.set(0.5);
      s.tint = hueFromSlot(slot);
      s.alpha = 0.85;
      s.scale.set(0.6);
      swarm.addChild(s);
      sprites.set(slot, s);
    }
    return s;
  }

  let stopped = false;
  app.ticker.add(() => {
    if (stopped) return;
    const w = app.screen.width;
    const h = app.screen.height;
    const scale = Math.min(w, h) / WORLD_SIZE;
    const me = selfSlot();
    const now = performance.now();

    const entities = world.interpolate(now);

    const alive = new Set<number>();
    for (const e of entities) {
      alive.add(e.slot);
      const s = spriteFor(e.slot);
      s.x = w / 2 + e.x * scale;
      s.y = h / 2 + e.z * scale;
      if (e.slot === me) {
        // Pull the self sprite into the foreground and make it
        // obviously different.
        s.tint = 0x66ccff;
        s.scale.set(1.3);
        s.alpha = 1.0;
        if (s.parent !== selfLayer) selfLayer.addChild(s);
      } else {
        const speed = Math.hypot(e.velX, e.velZ);
        // Slight wobble in scale with speed makes motion legible.
        s.scale.set(0.5 + Math.min(speed / 120, 0.4));
        if (s.parent !== swarm) swarm.addChild(s);
      }
    }
    for (const [slot, s] of sprites) {
      if (!alive.has(slot)) {
        s.destroy();
        sprites.delete(slot);
      }
    }
  });

  return () => {
    stopped = true;
    window.removeEventListener("resize", drawGrid);
    app.destroy(false, { children: true, texture: true });
  };
}
