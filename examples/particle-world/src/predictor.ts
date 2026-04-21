// Fixed-timestep client-side prediction with canonical server
// reconciliation. Combines:
//
//   - Gambetta "Client-Side Prediction and Server Reconciliation"
//     (Fast-Paced Multiplayer Part II): input buffer keyed by `seq`,
//     replay-on-reconcile.
//   - Fiedler "Fix Your Timestep": physics only advances in exact tick
//     increments; the renderer linearly interpolates between the two
//     most recent tick states for sub-tick visual smoothness.
//
// Why both? Pure Gambetta replay makes the predictor byte-for-byte
// correct against the server (same fixed-timestep discrete physics,
// same inputs) — eliminating the integration-mismatch drift that mixed
// continuous/discrete schemes cause during velocity ramps. Fiedler
// interpolation then maps the 30 Hz physics cadence to the 60 Hz render
// loop without stutter.
//
// Constants MUST match server-side `executor.rs` + `schema.rs`.

const ACCELERATION = 1000;
const MAX_VELOCITY = 250;
const WORLD_BOUND = 5000;
const DAMPING_PER_SECOND = 0.358;
/** Physics tick duration in seconds. Matches `DEMO_TICK_RATE_HZ = 30` in
 *  particle-server.rs. Identical to the server's `tick_dt_secs`. */
const TICK_DT_SEC = 1 / 30;
const TICK_PERIOD_MS = TICK_DT_SEC * 1000;

/**
 * Hard-snap on a gap this large between the predictor's latest tick
 * state and the server's authoritative state. In normal play the replay
 * produces a state byte-identical to what the server will eventually
 * report, so this only fires on out-of-band server events (teleport,
 * world-edge clamp, kick). 500 u is well beyond any legitimate physics
 * divergence at `MAX_VELOCITY = 250 u/s`.
 */
const HARD_SNAP_THRESHOLD = 500;

export type PredictedState = {
  posX: number;
  posZ: number;
  velX: number;
  velZ: number;
};

type PhysicsState = {
  posX: number;
  posZ: number;
  velX: number;
  velZ: number;
};

type PendingInput = {
  seq: number;
  dirX: number;
  dirZ: number;
  /** Local `performance.now()` when this input was recorded — i.e. the
   *  wall-clock moment at which the server's physics step applying this
   *  input is anchored for visual interpolation. */
  recordedAt: number;
};

function zero(): PhysicsState {
  return { posX: 0, posZ: 0, velX: 0, velZ: 0 };
}

export class SelfPredictor {
  /** Server-acknowledged state: last physics state whose inputs the
   *  server has confirmed applying. Updated by `reconcile`. */
  private synced: PhysicsState = zero();
  /** Inputs sent to the server but not yet acknowledged. Monotonic by seq. */
  private pending: PendingInput[] = [];
  /** Physics state at the `prev` tick boundary (N-1 applied inputs). */
  private prev: PhysicsState = zero();
  /** Physics state at the `curr` tick boundary (all pending inputs applied). */
  private curr: PhysicsState = zero();
  /** Local wall-clock time at which `curr` became authoritative (== when
   *  the latest pending input was recorded, or the reconcile moment if
   *  no pending inputs remain). Used as the `t=0` anchor for the
   *  interpolation between `prev` and `curr`. */
  private currBoundaryT: number | null = null;
  /** Last rendered state, returned by `current()`. */
  private displayed: PhysicsState = zero();

  /**
   * No-op retained for API compatibility with the input-loop rAF pump.
   * The discrete-physics model derives all motion from recorded inputs,
   * not from continuously-sampled held keys.
   */
  setInput(_dirX: number, _dirZ: number): void {
    // intentional no-op — see class docstring
  }

  /**
   * Apply exactly one server tick step on top of `curr` using the given
   * input direction. Called by input.ts each time a datagram is sent
   * (same cadence the server uses to process inputs: TICK_PERIOD_MS).
   */
  recordInput(seq: number, dirX: number, dirZ: number): void {
    const mag = Math.hypot(dirX, dirZ);
    const ndx = mag > 1e-5 && Number.isFinite(mag) ? dirX / mag : 0;
    const ndz = mag > 1e-5 && Number.isFinite(mag) ? dirZ / mag : 0;
    const now = performance.now();
    this.prev = { ...this.curr };
    this.curr = applyTickStep(this.curr, ndx, ndz);
    this.currBoundaryT = now;
    this.pending.push({ seq, dirX: ndx, dirZ: ndz, recordedAt: now });
  }

  /**
   * Rewind to server authoritative state, drop acknowledged inputs,
   * replay the remaining inputs as discrete tick steps. The replayed
   * `curr` is what the server would produce after processing the full
   * input stream — so no soft lerp is ever needed.
   */
  reconcile(
    serverX: number,
    serverZ: number,
    serverVX: number,
    serverVZ: number,
    lastProcessedSeq: number,
  ): void {
    const serverState: PhysicsState = {
      posX: serverX,
      posZ: serverZ,
      velX: serverVX,
      velZ: serverVZ,
    };

    // Catastrophic divergence: hard-reset. Happens only on server-forced
    // teleports / world-edge clamp / bugs, not normal play.
    const gapX = serverState.posX - this.curr.posX;
    const gapZ = serverState.posZ - this.curr.posZ;
    if (Math.hypot(gapX, gapZ) > HARD_SNAP_THRESHOLD) {
      this.synced = serverState;
      this.pending = [];
      this.prev = serverState;
      this.curr = serverState;
      this.currBoundaryT = performance.now();
      return;
    }

    this.synced = serverState;
    this.pending = this.pending.filter((p) => p.seq > lastProcessedSeq);

    // Replay remaining inputs as discrete tick steps on top of synced.
    // Track the state just before the last replayed input too (that's
    // our `prev` for visual interpolation).
    let replayPrev: PhysicsState = { ...serverState };
    let replayCurr: PhysicsState = { ...serverState };
    for (const input of this.pending) {
      replayPrev = { ...replayCurr };
      replayCurr = applyTickStep(replayCurr, input.dirX, input.dirZ);
    }
    this.prev = replayPrev;
    this.curr = replayCurr;

    // Anchor the interp timeline to when the last pending input was
    // recorded (best proxy for "when did curr become authoritative").
    // If no pending inputs remain, the displayed state equals synced
    // so the anchor is irrelevant — just use `now`.
    if (this.pending.length > 0) {
      this.currBoundaryT = this.pending[this.pending.length - 1].recordedAt;
    } else {
      this.currBoundaryT = performance.now();
    }
  }

  /**
   * Update the display state by linear interpolation between `prev` and
   * `curr` based on how much real time has elapsed since `curr` became
   * authoritative. `frac` clamps to [0, 1]:
   *   - 0 → just after the tick boundary: show `prev` blended to `curr`
   *   - 1 → a full tick has elapsed without a new input: show `curr`
   *
   * When there are no pending inputs, displayed = synced exactly.
   */
  advance(now: number): void {
    if (this.pending.length === 0 || this.currBoundaryT === null) {
      this.displayed = { ...this.synced };
      return;
    }
    const elapsedMs = now - this.currBoundaryT;
    let frac = elapsedMs / TICK_PERIOD_MS;
    if (frac < 0) frac = 0;
    if (frac > 1) frac = 1;
    this.displayed = {
      posX: this.prev.posX + (this.curr.posX - this.prev.posX) * frac,
      posZ: this.prev.posZ + (this.curr.posZ - this.prev.posZ) * frac,
      velX: this.prev.velX + (this.curr.velX - this.prev.velX) * frac,
      velZ: this.prev.velZ + (this.curr.velZ - this.prev.velZ) * frac,
    };
  }

  current(): PredictedState {
    return { ...this.displayed };
  }

  /** Number of inputs sent to the server but not yet acknowledged. */
  pendingCount(): number {
    return this.pending.length;
  }

  /** Highest seq currently in the pending buffer (0 if empty). */
  latestPendingSeq(): number {
    return this.pending.length === 0 ? 0 : this.pending[this.pending.length - 1].seq;
  }
}

/**
 * Mirrors `ParticleExecutor::call_handle_message` in executor.rs exactly.
 * Given a state and a unit-vector input direction, returns the state
 * after one `TICK_DT_SEC` physics step.
 */
function applyTickStep(
  state: PhysicsState,
  dirX: number,
  dirZ: number,
): PhysicsState {
  const hasInput = dirX !== 0 || dirZ !== 0;
  let velX = state.velX;
  let velZ = state.velZ;
  if (hasInput) {
    velX += dirX * ACCELERATION * TICK_DT_SEC;
    velZ += dirZ * ACCELERATION * TICK_DT_SEC;
  } else {
    const dampingPerTick = Math.pow(DAMPING_PER_SECOND, TICK_DT_SEC);
    velX *= dampingPerTick;
    velZ *= dampingPerTick;
  }
  const vmag = Math.hypot(velX, velZ);
  if (vmag > MAX_VELOCITY) {
    velX = (velX / vmag) * MAX_VELOCITY;
    velZ = (velZ / vmag) * MAX_VELOCITY;
  }
  return {
    velX,
    velZ,
    posX: clamp(state.posX + velX * TICK_DT_SEC, -WORLD_BOUND, WORLD_BOUND),
    posZ: clamp(state.posZ + velZ * TICK_DT_SEC, -WORLD_BOUND, WORLD_BOUND),
  };
}

function clamp(v: number, lo: number, hi: number): number {
  return v < lo ? lo : v > hi ? hi : v;
}
