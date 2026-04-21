// PixiJS renderer: WebGL-batched sprites at 60 fps with client-side
// velocity extrapolation between 20 Hz authoritative ticks. Camera
// follows the self sprite; self is rendered with a pulsing halo so
// you can never lose yourself in the swarm.

import { Application, Container, Graphics, Sprite } from "pixi.js";

import { SelfPredictor } from "./predictor";
import { WorldState } from "./state";

/** World units visible across the smaller viewport dimension. Lower
 *  numbers zoom in, making motion more obviously fast. */
const VIEWPORT_SPAN = 3000;

function hueFromSlot(slot: number): number {
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
  predictor: SelfPredictor,
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

  // Faint radial grid centered on the world origin, drawn in world-space
  // via a camera container so it scrolls with the player.
  const worldCamera = new Container();
  app.stage.addChild(worldCamera);

  const grid = new Graphics();
  worldCamera.addChild(grid);
  const drawGrid = () => {
    grid.clear();
    // Rings every 1000 world units from origin.
    for (let r = 500; r <= 5000; r += 500) {
      grid
        .circle(0, 0, r)
        .stroke({
          width: r % 1000 === 0 ? 2 : 1,
          color: 0x1f2a3a,
          alpha: r % 1000 === 0 ? 0.45 : 0.22,
        });
    }
    // Origin crosshair
    grid
      .moveTo(-5000, 0)
      .lineTo(5000, 0)
      .stroke({ width: 1, color: 0x1f2a3a, alpha: 0.3 });
    grid
      .moveTo(0, -5000)
      .lineTo(0, 5000)
      .stroke({ width: 1, color: 0x1f2a3a, alpha: 0.3 });
  };
  drawGrid();

  // Glowing disc texture reused by every entity sprite.
  const disc = new Graphics();
  disc.circle(16, 16, 5).fill({ color: 0xffffff, alpha: 1.0 });
  disc.circle(16, 16, 12).fill({ color: 0xffffff, alpha: 0.18 });
  const texture = app.renderer.generateTexture({
    target: disc,
    resolution: 2,
  });

  const swarm = new Container();
  swarm.blendMode = "add";
  worldCamera.addChild(swarm);

  const selfLayer = new Container();
  worldCamera.addChild(selfLayer);

  // Self halo — two concentric rings that pulse. In world-space so zoom
  // stays consistent.
  const halo = new Graphics();
  halo.visible = false;
  selfLayer.addChild(halo);

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
  let t = 0;
  // Smoothed camera position — chases the predicted self with an
  // exponential lerp so direction flips don't snap the world.
  let camX = 0;
  let camZ = 0;
  let camInitialized = false;
  app.ticker.add((ticker) => {
    if (stopped) return;
    t += ticker.deltaMS;
    const w = app.screen.width;
    const h = app.screen.height;
    const pxPerWorld = Math.min(w, h) / VIEWPORT_SPAN;
    const me = selfSlot();
    const now = performance.now();

    // Self: drive from client-side predictor so input feels instant.
    predictor.advance(now);
    const self = predictor.current();

    // Remote entities: render INTERP_DELAY_TICKS behind the latest
    // observed snapshot (tick-time based, not arrival-time based) so
    // network jitter doesn't feed into the lerp fraction. See
    // `state.ts` for the virtual-clock anchor algorithm.
    const remotes = world.interpolate(now, me);

    // Chase-cam: lerp toward self at a rate that feels locked-on but not
    // rubber-banded. Factor chosen so 99% of the delta closes in ~200 ms.
    const camLerp = 1 - Math.pow(0.01, ticker.deltaMS / 200);
    if (!camInitialized) {
      camX = self.posX;
      camZ = self.posZ;
      camInitialized = true;
    } else {
      camX += (self.posX - camX) * camLerp;
      camZ += (self.posZ - camZ) * camLerp;
    }
    worldCamera.x = w / 2 - camX * pxPerWorld;
    worldCamera.y = h / 2 - camZ * pxPerWorld;
    worldCamera.scale.set(pxPerWorld);

    const alive = new Set<number>();
    for (const e of remotes) {
      alive.add(e.slot);
      const s = spriteFor(e.slot);
      s.x = e.x;
      s.y = e.z;
      const speed = Math.hypot(e.velX, e.velZ);
      s.scale.set((0.55 + Math.min(speed / 500, 0.45)) / pxPerWorld);
      if (s.parent !== swarm) swarm.addChild(s);
    }
    // Self sprite: drawn from predicted state, always on top.
    if (me !== null) {
      alive.add(me);
      const s = spriteFor(me);
      s.x = self.posX;
      s.y = self.posZ;
      s.tint = 0xeeffff;
      s.scale.set(1.8 / pxPerWorld);
      s.alpha = 1.0;
      if (s.parent !== selfLayer) selfLayer.addChild(s);
    }
    for (const [slot, s] of sprites) {
      if (!alive.has(slot)) {
        s.destroy();
        sprites.delete(slot);
      }
    }

    // Pulsing halo around self. Two rings beating out of phase.
    if (me !== null) {
      halo.visible = true;
      halo.clear();
      const pulse1 = 70 + 30 * Math.sin(t * 0.004);
      const pulse2 = 110 + 40 * Math.sin(t * 0.004 + Math.PI);
      halo.x = self.posX;
      halo.y = self.posZ;
      halo
        .circle(0, 0, pulse1)
        .stroke({ width: 3 / pxPerWorld, color: 0x66ccff, alpha: 0.9 });
      halo
        .circle(0, 0, pulse2)
        .stroke({ width: 2 / pxPerWorld, color: 0x66ccff, alpha: 0.45 });
    } else {
      halo.visible = false;
    }
  });

  return () => {
    stopped = true;
    app.destroy(false, { children: true, texture: true });
  };
}
