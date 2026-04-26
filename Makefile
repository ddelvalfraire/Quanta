# Local dev launchers for the two browser demos.
#
# Particle-world: rust realtime-server (particle-server) + vite client.
# Quanta-code: phoenix server (apps/quanta_web) + vite client. NATS optional.
#
# Each "server" target runs in the foreground and blocks. Open a second
# terminal and run the corresponding "vite" target.

.PHONY: help setup \
        particle particle-server particle-server-empty particle-vite particle-wasm particle-clean \
        quanta-code quanta-code-vite phoenix \
        nats-up nats-down

# Match the dev token hardcoded in examples/quanta-code/src/connection.ts so the
# editor's Phoenix socket handshake authenticates without env-var gymnastics.
DEV_API_KEY := qk_rw_dev_devdevdevdevdevdevdevdevdevdevde

# wasm-pack picks up rustc from PATH; homebrew rustc lacks the wasm32 target.
# Force the rustup toolchain for the wasm build only.
RUSTUP_RUSTC := $(HOME)/.cargo/bin/rustc
RUSTUP_CARGO := $(HOME)/.cargo/bin/cargo

# How many NPCs the particle-world server spawns. Visible swarm by default;
# override for stress runs: `make particle-server NPC_COUNT=1000`.
NPC_COUNT ?= 300

help:
	@echo "One-command demo launchers (Ctrl-C tears down both processes):"
	@echo "  make particle            particle-world: realtime server + browser client"
	@echo "  make quanta-code         editor: phoenix server + browser client"
	@echo ""
	@echo "Setup:"
	@echo "  make setup               install wasm-pack + npm deps (one-time)"
	@echo ""
	@echo "Individual processes (run in separate terminals if you want them apart):"
	@echo "  make particle-server         realtime server + $(NPC_COUNT) NPCs (127.0.0.1:4443)"
	@echo "  make particle-server-empty   realtime server with no NPCs"
	@echo "  make particle-vite           particle-world browser client (:5173)"
	@echo "  make particle-wasm           rebuild wasm decoder (after wire-format changes)"
	@echo "  make particle-clean          wipe wasm-decoder build output"
	@echo "  make phoenix                 umbrella web server (:4000)"
	@echo "  make quanta-code-vite        editor browser client (:5173)"
	@echo "  make nats-up / nats-down     start/stop single NATS node via docker compose"

setup:
	@command -v wasm-pack >/dev/null || cargo install wasm-pack
	cd examples/particle-world && npm install
	cd examples/quanta-code && npm install

# ---------------------------------------------------------------- particle

# One-command launcher. Backgrounds both processes under a single shell
# so Ctrl-C reaches the whole process group via the trap.
particle:
	@trap 'kill 0' INT TERM; \
	$(MAKE) -s particle-server & \
	$(MAKE) -s particle-vite & \
	wait

# Skips the wasm rebuild that `npm run dev` would do. Run `make particle-wasm`
# explicitly when the wire format changes.
particle-vite:
	cd examples/particle-world && npx vite

particle-server:
	cd rust && QUANTA_NPC_COUNT=$(NPC_COUNT) cargo run --release -p quanta-particle-demo --bin particle-server

# Bare server with no NPCs — only useful for cross-tab self-entity testing.
particle-server-empty:
	cd rust && cargo run --release -p quanta-particle-demo --bin particle-server

particle-wasm:
	cd examples/particle-world && \
		PATH="$(HOME)/.cargo/bin:$$PATH" \
		RUSTC="$(RUSTUP_RUSTC)" CARGO="$(RUSTUP_CARGO)" \
		npm run build:wasm

particle-clean:
	rm -rf examples/particle-world/wasm-decoder

# ---------------------------------------------------------------- quanta-code

# One-command launcher.
quanta-code:
	@trap 'kill 0' INT TERM; \
	$(MAKE) -s phoenix & \
	$(MAKE) -s quanta-code-vite & \
	wait

# Generates dev secrets each invocation. Sessions don't persist across
# restarts anyway, so there's nothing to lose.
phoenix:
	@QUANTA_WASM_HMAC_KEY=$$(openssl rand -hex 32) \
	QUANTA_SECRET_KEY_BASE=$$(openssl rand -base64 64 | tr -d '\n') \
	QUANTA_API_KEYS="$(DEV_API_KEY)" \
	MIX_ENV=dev mix phx.server

quanta-code-vite:
	cd examples/quanta-code && npx vite

# Optional. The umbrella boots without NATS but logs reconnect noise and
# cross-node sync is degraded.
nats-up:
	docker compose -f docker-compose.test.yml up -d nats-1

nats-down:
	docker compose -f docker-compose.test.yml down
