import type { DecodedState } from "@quanta/delta-decoder";
import type { DebugOptions } from "./types.js";

export interface DebugMetrics {
  deltaCount: number;
  totalDeltaBytes: number;
  avgDecodeLatencyMs: number;
  maxDecodeLatencyMs: number;
  connectionCount: number;
}

type Verbosity = "minimal" | "normal" | "verbose";

export class DebugLogger {
  private enabled: boolean;
  private verbosity: Verbosity;

  private deltaCount = 0;
  private totalDeltaBytes = 0;
  private totalDecodeLatencyMs = 0;
  private maxDecodeLatencyMs = 0;
  private _connectionCount = 0;

  constructor(opts?: DebugOptions) {
    this.enabled = opts?.enabled ?? false;
    this.verbosity = opts?.verbosity ?? "normal";
  }

  log(level: Verbosity, ...args: unknown[]): void {
    if (!this.enabled) return;
    if (!this.shouldLog(level)) return;
    console.log(`[quanta]`, ...args);
  }

  recordDelta(
    entitySlot: number,
    deltaSize: number,
    decodeLatencyMs: number,
    decodedState?: DecodedState,
  ): void {
    this.deltaCount++;
    this.totalDeltaBytes += deltaSize;
    this.totalDecodeLatencyMs += decodeLatencyMs;
    if (decodeLatencyMs > this.maxDecodeLatencyMs) {
      this.maxDecodeLatencyMs = decodeLatencyMs;
    }

    if (this.enabled && this.shouldLog("verbose") && decodedState) {
      console.log(
        `[quanta] delta slot=${entitySlot} size=${deltaSize}B latency=${decodeLatencyMs.toFixed(2)}ms`,
        JSON.stringify(decodedState),
      );
    }
  }

  recordConnection(): void {
    this._connectionCount++;
  }

  getMetrics(): DebugMetrics {
    return {
      deltaCount: this.deltaCount,
      totalDeltaBytes: this.totalDeltaBytes,
      avgDecodeLatencyMs:
        this.deltaCount > 0
          ? this.totalDecodeLatencyMs / this.deltaCount
          : 0,
      maxDecodeLatencyMs: this.maxDecodeLatencyMs,
      connectionCount: this._connectionCount,
    };
  }

  private shouldLog(level: Verbosity): boolean {
    const levels: Verbosity[] = ["minimal", "normal", "verbose"];
    return levels.indexOf(level) <= levels.indexOf(this.verbosity);
  }
}
