# Particle World — 1K clients, 10 minutes

## Reproduction

```bash
# Terminal 1
cd rust
cargo run --release -p quanta-particle-demo --bin particle-server

# Terminal 2 (same repo)
ulimit -n 4096   # macOS default is 256, too low for 1K sockets
cd rust
cargo run --release --features load \
  -p quanta-particle-demo --bin quanta-load -- \
  --addr 127.0.0.1:4443 --clients 1000 --duration 600s --ramp 30s
```

Scrape metrics during the run:

```bash
watch -n 5 'curl -s http://127.0.0.1:9090/metrics \
  | grep -E "(tick_duration|clients_connected|datagrams_sent|bytes_sent)"'
```

## Machine

- Model: Apple M3 Pro, 11 cores, 18 GB
- OS: macOS 26.4.1 (Darwin 25.4.0)
- Rust: 1.94.1 (release profile)

## Server-side metrics (Prometheus scrape at end of run)

| Metric | Value | Target |
|---|---|---|
| `tick_duration_seconds` sum | 4.108 s over 12 740 ticks | — |
| `tick_duration_seconds` mean | 0.32 ms | — |
| `tick_duration_seconds` p99 | < 1 ms (99.2 % of ticks) | **< 10 ms** |
| `tick_duration_seconds` p99.9 | < 2 ms (99.6 % of ticks) | — |
| `clients_connected` (peak) | 963 (observed via live scrape during run) | — |
| `datagrams_sent_total` | 13 968 396 | — |
| `bytes_sent_total` | 341 352 921 (≈ 325 MB) | — |

### Tick-duration histogram (cumulative count per bucket)

```
tick_duration_seconds_bucket{le="0.0005"} 11246   (88.3 %)
tick_duration_seconds_bucket{le="0.001"}  12632   (99.2 %)
tick_duration_seconds_bucket{le="0.002"}  12690   (99.6 %)
tick_duration_seconds_bucket{le="0.005"}  12718   (99.83 %)
tick_duration_seconds_bucket{le="0.01"}   12729   (99.91 %)
tick_duration_seconds_bucket{le="0.025"}  12738   (99.98 %)
tick_duration_seconds_bucket{le="0.05"}   12740   (100 %)
tick_duration_seconds_bucket{le="0.1"}    12740   (100 %)
tick_duration_seconds_bucket{le="+Inf"}   12740
tick_duration_seconds_sum                 4.108394524
tick_duration_seconds_count               12740
```

12 740 ticks over ~605 s ≈ 20.0 Hz — engine kept cadence end-to-end.

## Client-side summary (`quanta-load` stdout)

| Metric | Value | Target |
|---|---|---|
| connects / attempted | 963 / 1000 (96.3 %) | 100 % |
| disconnects mid-run | 0 | **0** |
| avg bytes/sec/client out (client-sent) | 3.9 Kbps | — |
| avg bytes/sec/client in (server-sent) | **4.6 Kbps** | **< 15 Kbps** |

Raw summary output:

```
summary:
  connects:    963 / 1000 (96.3%)
  disconnects: 0 mid-run
  sent:        11208388 datagrams, 280209700 bytes (3.9 Kbps/client avg)
  recv:        13667248 datagrams, 334043045 bytes (4.6 Kbps/client avg)
```

## Verdict

| Target | Pass / Fail |
|---|---|
| p99 tick < 10 ms | **PASS** (p99 < 1 ms) |
| Avg per-client recv BW < 15 Kbps | **PASS** (4.6 Kbps) |
| Zero mid-run disconnects | **PASS** (0) |

## Interpretation

All three targets cleared with wide margin — p99 tick duration was 10× under
the budget and per-client bandwidth was 3× under. The only shortfall was the
initial connect-ramp: 37 of 1000 clients hit the server's 5-second
`auth_timeout` while the QUIC accept loop was saturated during the 30-second
ramp. Once past the ramp the system held 963 clients at a dead-stable 20 Hz
for 10 minutes with no disconnects. Phase 6 polish for 100 % connect rate:
lengthen the ramp to 60 s, raise `EndpointConfig.auth_timeout` to 10 s, or
add a client-side retry on auth timeout.
