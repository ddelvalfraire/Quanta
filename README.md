# Quanta

A distributed multiplayer platform for browser-based realtime applications. Quanta provides the actor runtime, networking, and state-replication primitives so applications can focus on game/feature logic, not on the systems plumbing that makes 300+ concurrent clients feel snappy from a laptop.

The repository contains the platform itself plus two reference applications built on top of it:

- **particle-world** — a 30 Hz authoritative simulation streaming up to thousands of entities to a WebTransport (QUIC) browser client with client-side prediction.
- **quanta-code** — a real-time collaborative code editor with sub-document CRDTs, presence, and a sandboxed JS execution surface, served over Phoenix WebSockets.

Both apps share the same actor runtime, schema-driven binary wire format, and supervision strategy.

---

## Project goals

1. **Show that a heterogeneous system can still be coherent.** Quanta deliberately uses three runtimes — Elixir (BEAM), Rust (native + WebAssembly), and TypeScript — and bridges them with one wire protocol and one schema definition. The goal is to prove that the right tool per layer beats forcing one language across the stack.
2. **Take "soft realtime" seriously.** Sub-100 ms perceived latency is a systems problem, not a frontend problem. The platform commits to fixed-timestep server simulation, client-side prediction with server reconciliation, and snapshot interpolation — all the pieces fast-paced multiplayer games have used for decades but rarely see in browser apps.
3. **Make failure boring.** Supervision trees, drain hooks, mailbox-shed thresholds, hybrid logical clocks, and idempotent reconnect tokens are not afterthoughts — they are first-class architectural decisions baked into the supervision graph.

---

## Course themes

The project is built around four systems-programming themes, integrated rather than bolted on:

### 1. Concurrency and the process model

The actor runtime in `apps/quanta_distributed/lib/quanta/actor/` is a classical share-nothing concurrent system: every actor is a BEAM process with a private mailbox, and the runtime hosts thousands of them under a partitioned `DynamicSupervisor` (`Quanta.Actor.DynSup`). Each actor's `Server` GenServer enforces mailbox-shed thresholds (1k warn / 5k shed / 10k critical) so a slow consumer cannot starve the scheduler. On the Rust side, the realtime server in `rust/quanta-realtime-server/` runs an island-per-tick loop over the Tokio multi-threaded async runtime; per-island state is owned by a single task to avoid lock contention on the hot path.

### 2. Inter-process communication and networking

Three transports coexist:

- **WebTransport (QUIC)** — primary realtime transport for unreliable-but-fast datagrams to the browser. The `delta_envelope` module defines a compact framed wire format with flags for full-state vs. delta vs. welcome vs. seq-ack heartbeats.
- **WebSocket** — fallback transport (browsers/networks without WebTransport) and the primary path for the Phoenix-channel-based collaborative editor.
- **NATS** — cluster-internal pub/sub between Elixir nodes for command routing and bridge subscriptions, ingested through Broadway pipelines for backpressure.

The wire protocol is schema-driven: the same Rust crate (`quanta-core-rs::delta::encoder`) that encodes deltas on the server is compiled to WebAssembly and runs unchanged in the browser, eliminating the JS-port-drift class of bug.

### 3. Synchronization, fault tolerance, and distributed coordination

The top-level supervisor uses `:rest_for_one` so an infrastructure failure (e.g., catastrophic NATS loss) restarts the actor runtime, but an actor-layer crash cannot recycle cluster-critical services — an asymmetry the codebase comments on explicitly. Cross-node coordination uses `syn` for distributed process registration and a hybrid logical clock (`Quanta.Hlc`) for total event ordering across nodes without a wall-clock dependency. Rate limiting is per-actor and tracked in a sliding window. Drain controllers expose `/api/internal/drain` so an operator can pause incoming work before terminating a node.

### 4. Systems integration and performance

Hot paths that are not viable in pure BEAM cross into Rust through Rustler NIFs (`apps/quanta_nifs/` ↔ `rust/quanta-nifs/`): the Loro CRDT engine, schema compilation, the delta encoder, an ephemeral KV store, and a Wasmtime runtime that sandbox-executes user code. The same Rust crate is reused on the realtime server and compiled to wasm for the browser client, so a state delta produced by the server can be applied byte-identically by the predictor in the browser. Delta encoding ships only changed fields with quantization; full-state snapshots are reserved for new clients or schema-version mismatches.

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
| `rust/quanta-core-rs` | Schema, delta encoder, bridge codec — used by both NIFs and wasm |
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

- **Erlang/OTP 26+**, **Elixir 1.16+**
- **Rust 1.75+** with the `wasm32-unknown-unknown` target (`rustup target add wasm32-unknown-unknown`)
- **Node.js 20+** and **npm**
- **wasm-pack** (`cargo install wasm-pack`) — installed automatically by `make setup`
- Optional: **Docker** (for the optional NATS node)

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

Once the server is running, open `http://127.0.0.1:5173/` in a Chromium-based browser (WebTransport requires Chrome/Edge/Opera; Firefox will fall back to WebSocket where supported).

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

- **BEAM (Elixir/OTP)** for the actor runtime — preemptive scheduling and battle-tested supervision are exactly what an actor system needs, and writing a million-process scheduler from scratch in Rust is not a course-project budget.
- **Rust** for the realtime path and CRDT/delta hot paths — predictable latency, no GC pauses, and the option to ship the same code to the browser as wasm.
- **TypeScript** for the browser — the only realistic option, but kept thin: rendering, input, and predictor only. All decoding happens in wasm.

The cost is a polyglot toolchain. The benefit is that no layer is fighting its language: BEAM does what BEAM is for, Rust does what Rust is for, and the browser only renders.

### Why a custom binary wire format instead of JSON or Protobuf?

For 300 entities at 30 Hz, the wire is the bottleneck — JSON would ~10× the bytes per tick and protobuf would not let us share encoder code with the browser. The current schema-driven encoder quantizes per-field, sends only changed fields, and supports rolling schema versions. It is built once in Rust and consumed natively (server) and via wasm (browser) so the wire format has exactly one implementation.

### Why fixed-timestep server simulation with client-side prediction?

The two non-negotiables for fast-paced multiplayer are determinism (server replays match predictor replays byte-for-byte) and decoupled rendering (60 fps render over 30 Hz physics). The predictor uses Gambetta's input-buffer/replay scheme and Fiedler's fixed-timestep + interpolation pattern — these are decades-old techniques because they work, and rolling something simpler always ends in jitter.

### Why `:rest_for_one` at the top of the supervision tree?

Infrastructure (NATS, syn, HLC) starting before the actor runtime, with `:rest_for_one`, guarantees that an actor-layer bug cannot recycle cluster-critical services. The asymmetry is intentional: bad app code should never look like a node failure to the rest of the cluster.

### Trade-offs we accepted

- **No persistent durable storage yet** — actors are in-memory CRDTs; persistence is on the roadmap but out of scope for the course project.
- **Self-signed certs for WebTransport in dev** — the cert hash is published via `/server-info.json` rather than going through a real CA. Production would integrate Let's Encrypt or equivalent.
- **NATS is single-node in dev** — the cluster topology code exists but is exercised against one node locally; full multi-node replication testing needs more time than the project window.
- **Thin written README at submission time** — the codebase is heavily commented at decision points, so much of the rationale lives next to the code rather than in standalone docs. This README is the consolidated version.

---

## Challenges and lessons learned

### Snapshot interpolation jitter

The first version of the client interpolated snapshots using local arrival timestamps. Motion looked terrible — frequent micro-stutters even though the server was sending smooth state at 30 Hz. The fix (in `examples/particle-world/src/state.ts`) was to interpolate against the *server tick* timeline rather than local arrival time, then nudge a virtual server clock toward each new snapshot's `(tick, arrival)` pair. Lesson: jitter is almost always a clock-domain mismatch problem, not a sample-rate problem.

### Predictor / server divergence

Early versions of the client predictor used continuous-time integration while the server used fixed-timestep integration. Even at 30 Hz the two diverged enough during velocity ramps that reconciliation produced visible rubber-banding. Switching the predictor to the *exact* same fixed-timestep discrete physics as the server (same `tick_dt_secs`, same damping math, same constants) made the replay byte-identical against the server, eliminating reconciliation snaps for normal play. Lesson: client and server must be byte-equivalent for prediction to work — "close enough" is not close enough.

### Supervision-tree boot ordering

Originally `Syn.add_node_to_scope/2` was called from the `Application` callback. On a scope conflict (e.g., two applications fighting for the same scope), the call would raise inside the OTP boot path and the node would refuse to start. Pushing syn setup *under* `Quanta.Supervisor` instead of into the Application callback keeps boot resilient. Lesson: never put unbounded-can-fail work in the Application callback — its only job is to start the top supervisor.

### Mailbox shedding

Under load, a single slow actor would back up its mailbox, which back-pressured every NATS subscription pinned to that actor, which made the entire pipeline appear to stall. The actor server now has tiered mailbox thresholds (1k warn, 5k shed non-essential messages, 10k critical) and emits telemetry at each tier. Lesson: in any system with unbounded queues, one of the first things to add is a bounded-queue policy with explicit drop semantics.

### Cross-language schema drift

When the wire format was edited on the server side, the wasm decoder in the browser kept compiling fine and silently misinterpreting bytes. We now ship a single Rust encoder/decoder crate (`quanta-core-rs::delta::encoder`) that is compiled to both native and wasm — a wire-format edit becomes a compile error in both consumers simultaneously. Lesson: in a polyglot system, the only honest contract is shared *code*, not a shared *spec*.

---

## License

Source-available for evaluation in the context of CPT_S 360. All rights reserved beyond that scope.
