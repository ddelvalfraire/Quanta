# Quanta

Distributed multiplayer platform for browser-based realtime apps. The repo holds the platform plus two reference apps built on it:

- **particle-world** — 30 Hz authoritative simulation streaming a few thousand entities to a WebTransport (QUIC) browser client with client-side prediction.
- **quanta-code** — realtime collaborative code editor with CRDTs, presence, and a sandboxed JS executor, served over Phoenix WebSockets.

Both apps share the same actor runtime, schema-driven binary wire format, and supervision strategy.

---

## Project goals

This is a course project but also the platform I'd actually want to use. Three things drove the design:

1. Polyglot can be coherent. Quanta uses three runtimes (Elixir/BEAM, Rust native + wasm, TypeScript) bridged by one wire protocol and one schema. The point is to show that picking the right tool per layer beats forcing one language across the whole stack.
2. Soft realtime is a systems problem. Sub-100 ms perceived latency in the browser needs fixed-timestep server simulation, client-side prediction with reconciliation, and snapshot interpolation. None of those are frontend tricks; they're the same techniques fast-paced multiplayer games have shipped for 20+ years.
3. Failure should be boring. Supervisors, drain endpoints, mailbox shedding, hybrid logical clocks, idempotent reconnect tokens. All baked into the supervision graph from day one rather than retrofitted later.

---

## Course themes

Four systems-programming themes drive the architecture:

### 1. Concurrency and the process model

The actor runtime in `apps/quanta_distributed/lib/quanta/actor/` is a share-nothing concurrent system. Each actor is a BEAM process with a private mailbox, and the runtime hosts thousands of them under a partitioned `DynamicSupervisor` (`Quanta.Actor.DynSup`). The `Server` GenServer enforces mailbox-shed thresholds (1k warn, 5k shed, 10k critical) so a slow actor can't starve the scheduler.

On the Rust side, `rust/quanta-realtime-server/` runs a per-island tick loop on the Tokio multi-threaded async runtime. Per-island state is owned by a single task to avoid lock contention on the hot path.

### 2. IPC and networking

Three transports coexist:

- WebTransport (QUIC) is the primary realtime transport. The `delta_envelope` module defines a compact framed format with flags for full-state, delta, welcome, and seq-ack heartbeats.
- WebSocket is the fallback for browsers without WebTransport, and the primary path for the collaborative editor (Phoenix Channels).
- NATS handles cluster-internal pub/sub between Elixir nodes, ingested through Broadway pipelines for backpressure.

The wire protocol is schema-driven. The same Rust crate (`quanta-core-rs::delta::encoder`) that encodes deltas on the server runs in the browser too, compiled to wasm.

### 3. Synchronization, fault tolerance, distributed coordination

The top-level supervisor uses `:rest_for_one`. A catastrophic NATS loss restarts the actor runtime, but actor-layer crashes can't recycle cluster-critical infrastructure. That asymmetry is intentional and the codebase comments on it.

Cross-node coordination uses `syn` for distributed process registration and a hybrid logical clock (`Quanta.Hlc`) for total event ordering across nodes without depending on wall-clock time. Rate limiting is per-actor over a sliding window. There's a drain endpoint (`/api/internal/drain`) so an operator can pause incoming work before terminating a node.

### 4. Systems integration and performance

Hot paths that don't fit pure BEAM cross into Rust through Rustler NIFs (`apps/quanta_nifs/` ↔ `rust/quanta-nifs/`): the Loro CRDT engine, schema compilation, the delta encoder, an ephemeral KV store, and a Wasmtime runtime that sandbox-executes user code.

The same Rust crate is shared between the realtime server and the browser client (compiled to wasm). A delta produced on the server is applied bit-for-bit by the predictor in the browser. Delta encoding ships only the fields that changed, with per-field quantization. Full-state snapshots are reserved for new clients and schema-version mismatches.

---

## System architecture

```
                       browser                                cluster
       ┌─────────────────────────────────┐         ┌────────────────────────────────┐
       │ examples/particle-world         │  QUIC   │ rust/quanta-realtime-server    │
       │   wasm decoder + predictor      │ ──────► │   tick engine, fanout, pacing  │
       │                                 │         │   per-island Tokio task        │
       └─────────────────────────────────┘         └──────────────┬─────────────────┘
                                                                  │ NATS
       ┌─────────────────────────────────┐  WS     ┌──────────────▼─────────────────┐
       │ examples/quanta-code            │ ──────► │ apps/quanta_web (Phoenix)      │
       │   collaborative editor          │         │   ActorSocket / Channels       │
       └─────────────────────────────────┘         └──────────────┬─────────────────┘
                                                                  │
                                                   ┌──────────────▼─────────────────┐
                                                   │ apps/quanta_distributed         │
                                                   │   Actor.Supervisor (BEAM)       │
                                                   │   ├── DynSup → Server (×N)      │
                                                   │   ├── CommandRouter             │
                                                   │   └── Bridge.Subscriptions      │
                                                   └──────────────┬─────────────────┘
                                                                  │ Rustler NIFs
                                                   ┌──────────────▼─────────────────┐
                                                   │ rust/quanta-nifs                │
                                                   │   loro_engine, delta_encoder,   │
                                                   │   wasm_runtime, schema_compiler │
                                                   └────────────────────────────────┘
```

Repository layout:

| Path | Role |
|------|------|
| `apps/quanta_core` | Pure domain types (Actor behaviour, Envelope, Effect, ActorId, Manifest, HLC) |
| `apps/quanta_distributed` | Distributed actor runtime: supervisors, registry, NATS, Broadway, drain |
| `apps/quanta_nifs` | Rustler-NIF Elixir wrappers for Rust hot-path code |
| `apps/quanta_web` | Phoenix endpoint, channels, REST control plane |
| `rust/quanta-core-rs` | Schema, delta encoder, bridge codec — used by NIFs and wasm |
| `rust/quanta-nifs` | Native Rust implementations behind the NIF boundary |
| `rust/quanta-realtime-server` | Standalone QUIC + WS realtime server (tick engine, fanout, pacing) |
| `rust/quanta-particle-demo` | Particle-world product code (executor + fanout factories) |
| `rust/quanta-wasm-decoder` | wasm-bindgen build of the delta encoder for browsers |
| `packages/client` | TypeScript client for the Phoenix track (channels, schema cache) |
| `packages/delta-decoder` | TS wrapper around the wasm decoder |
| `examples/particle-world` | WebTransport browser client (renderer, predictor, input loop) |
| `examples/quanta-code` | Collaborative-editor browser client |

---

## Build instructions

### Prerequisites

- Erlang/OTP 26+, Elixir 1.16+
- Rust 1.75+ with the `wasm32-unknown-unknown` target (`rustup target add wasm32-unknown-unknown`)
- Node.js 20+ and npm
- wasm-pack (`cargo install wasm-pack`) — installed automatically by `make setup`
- Optional: Docker, for the optional NATS node

### One-time setup

```bash
make setup           # installs wasm-pack, npm deps for both demo apps
mix deps.get         # Elixir dependencies
mix compile          # compiles the umbrella + Rustler NIFs
```

### Run the demos

Each demo runs end-to-end with one command (Ctrl-C tears down both processes):

```bash
make particle        # particle-world: realtime server (127.0.0.1:4443) + browser client (127.0.0.1:5173)
make quanta-code     # quanta-code: phoenix server (:4000) + browser client (:5173)
```

Useful variants:

```bash
make particle NPC_COUNT=1000   # scale the swarm
make particle-server-empty     # realtime server with no NPCs (interactive only)
make particle-wasm             # rebuild the wasm decoder after wire-format changes
make nats-up / nats-down       # optional single-node NATS via docker compose
```

Once the server is running, open `http://127.0.0.1:5173/` in a Chromium-based browser. WebTransport requires Chrome/Edge/Opera; Firefox falls back to WebSocket where supported.

### Run the tests

```bash
mix test                                       # full umbrella test suite
cd rust/quanta-realtime-server && cargo test   # realtime server unit + integration
cd packages/client && npm test                 # TS client (vitest)
cd examples/particle-world && npm test         # client-side prediction + state sync
```

---

## Design decisions and trade-offs

### Why three runtimes?

- BEAM (Elixir/OTP) for the actor runtime. Preemptive scheduling and OTP supervision are what an actor system actually needs, and reimplementing a million-process scheduler from scratch in Rust is not a course-project budget.
- Rust for the realtime server and CRDT/delta hot paths. Predictable latency, no GC pauses, and the same code can ship to the browser as wasm.
- TypeScript for the browser. Realistically the only option, but kept thin: rendering, input, predictor. All decoding happens in wasm.

The cost is a polyglot toolchain. The benefit is that no layer is doing something it's bad at.

### Why a custom binary wire format?

For 300 entities at 30 Hz, the wire is the bottleneck. JSON would roughly 10× the bytes per tick. Protobuf would mean two implementations of the encoder, which is exactly what we wanted to avoid. The current format is schema-driven, quantizes per-field, ships only changed fields, and supports rolling schema versions. It's defined once in Rust and consumed natively (server) and via wasm (browser).

### Why fixed-timestep simulation with client-side prediction?

Fast-paced multiplayer needs determinism — server replays match predictor replays bit-for-bit — and decoupled rendering at 60 fps over 30 Hz physics. The predictor uses Gambetta's input-buffer/replay scheme and Fiedler's fixed-timestep + interpolation pattern. These are 20-year-old techniques that work; rolling something simpler ends in jitter every time.

### Why `:rest_for_one` at the top of the supervision tree?

Infrastructure (NATS, syn, HLC) starts before the actor runtime. With `:rest_for_one`, an actor-layer bug can't recycle cluster-critical services. Bad app code shouldn't look like a node failure to the rest of the cluster.

### Trade-offs accepted

- No persistent durable storage yet. Actors are in-memory CRDTs. Persistence is on the roadmap, out of scope here.
- Self-signed certs for WebTransport in dev. The cert hash is published via `/server-info.json` rather than going through a CA. Production would integrate Let's Encrypt or similar.
- NATS is single-node in dev. The cluster topology code exists but is only exercised against one node locally.
- Most of the design rationale lives in comments next to the code; this README is the consolidated version.

---

## Challenges and lessons learned

### Snapshot interpolation jitter

The first version of the client interpolated snapshots using local arrival timestamps. Motion looked terrible — micro-stutters everywhere even though the server was sending smooth state at 30 Hz. The fix (`examples/particle-world/src/state.ts`) was to interpolate against the server tick timeline instead of local arrival time, then nudge a virtual server clock toward each new snapshot's `(tick, arrival)` pair. Most jitter problems in this kind of system turn out to be clock-domain mismatches, not sample-rate problems.

### Predictor / server divergence

Early versions of the predictor used continuous-time integration while the server used fixed-timestep. Even at 30 Hz they diverged enough during velocity ramps that reconciliation produced visible rubber-banding. Switching the predictor to use the exact same fixed-timestep discrete physics as the server (same `tick_dt_secs`, same damping math, same constants) made the replay bit-identical against the server, and reconciliation snaps disappeared for normal play. Close-enough is not close enough for prediction.

### Supervision-tree boot ordering

Originally `Syn.add_node_to_scope/2` was called from the `Application` callback. On a scope conflict (two apps fighting for the same scope), the call would raise inside the OTP boot path and the node would refuse to start. Moving syn setup under `Quanta.Supervisor` instead of into the Application callback keeps boot resilient. The Application callback's only job should be to start the top supervisor.

### Mailbox shedding

Under load, a single slow actor would back up its mailbox, which back-pressured every NATS subscription pinned to that actor, which made the whole pipeline appear stalled. The actor server now has tiered mailbox thresholds (1k warn, 5k shed non-essential, 10k critical) and emits telemetry at each tier. In any system with unbounded queues, a bounded-queue policy with explicit drop semantics has to be there from the start.

---

## License

Source-available for evaluation in the context of CPT_S 360. All rights reserved beyond that.
