# Particle World — browser demo

Chromium-only proof-of-concept client for the Quanta realtime server.
Opens a WebTransport connection to `particle-server`, authenticates, and
renders every entity the fanout sends. WASD in one tab moves a dot in
every other connected tab.

## One-time setup

```bash
cargo install wasm-pack      # builds the JS-facing wasm module
cd examples/particle-world
npm install
```

## Run

Two terminals.

### Terminal 1 — server

```bash
cd rust
cargo run -p quanta-particle-demo --bin particle-server
```

On startup the server writes a fresh `server-info.json` into
`examples/particle-world/public/` containing the QUIC listen address,
the SHA-256 of its self-signed cert (browser uses it via
`serverCertificateHashes`), and the compiled entity schema bytes.

### Terminal 2 — Vite dev server

```bash
cd examples/particle-world
npm run dev
```

Open `http://localhost:5173` in **two** Chrome tabs. Click into the first
tab and press WASD — both tabs' blue circles drift in sync.

## Known limitations

- **Chromium only**. Firefox and Safari don't support
  `WebTransport.serverCertificateHashes` or drop datagrams entirely.
- **Self-signed cert validity is 13 days**. The server regenerates it on
  every restart, so restarting the server invalidates open tabs — just
  refresh the browser.
- **No client-side prediction**. Rendering is fully authoritative;
  visible input lag is the real server round-trip.
- **Demo-quality UI**. Blue dot = self, gray dots = others. Single
  screen, no smoothing, no mini-map — intentionally minimal.

## Troubleshooting

- `fetch /server-info.json failed: 404` — the Rust server isn't running
  or the relative default path didn't resolve. Start the server from the
  `rust/` directory, or set `QUANTA_SERVER_INFO_FILE` to an absolute path.
- `WebTransportError: Opening handshake failed` — the cert hash in
  `server-info.json` is stale (server restarted since the tab loaded).
  Refresh the tab.
- `auth rejected: invalid_token` — the server's `DEFAULT_DEV_TOKEN`
  doesn't match `examples/particle-world/src/main.ts`'s hardcoded value.
  Update one.
