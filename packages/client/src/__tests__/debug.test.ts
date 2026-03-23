import { describe, it, expect, vi } from "vitest";
import { DebugLogger } from "../debug.js";

describe("DebugLogger", () => {
  it("does not log when disabled", () => {
    const spy = vi.spyOn(console, "log").mockImplementation(() => {});
    const logger = new DebugLogger({ enabled: false });
    logger.log("minimal", "test");
    expect(spy).not.toHaveBeenCalled();
    spy.mockRestore();
  });

  it("logs when enabled", () => {
    const spy = vi.spyOn(console, "log").mockImplementation(() => {});
    const logger = new DebugLogger({ enabled: true, verbosity: "normal" });
    logger.log("normal", "hello");
    expect(spy).toHaveBeenCalledWith("[quanta]", "hello");
    spy.mockRestore();
  });

  it("filters by verbosity level", () => {
    const spy = vi.spyOn(console, "log").mockImplementation(() => {});
    const logger = new DebugLogger({ enabled: true, verbosity: "normal" });

    logger.log("minimal", "shown");
    logger.log("normal", "shown");
    logger.log("verbose", "hidden");

    expect(spy).toHaveBeenCalledTimes(2);
    spy.mockRestore();
  });

  it("tracks delta metrics", () => {
    const logger = new DebugLogger();

    logger.recordDelta(0, 100, 0.5);
    logger.recordDelta(1, 200, 1.5);
    logger.recordDelta(0, 50, 0.3);

    const metrics = logger.getMetrics();
    expect(metrics.deltaCount).toBe(3);
    expect(metrics.totalDeltaBytes).toBe(350);
    expect(metrics.avgDecodeLatencyMs).toBeCloseTo(0.767, 2);
    expect(metrics.maxDecodeLatencyMs).toBe(1.5);
  });

  it("returns zero avg when no deltas recorded", () => {
    const logger = new DebugLogger();
    expect(logger.getMetrics().avgDecodeLatencyMs).toBe(0);
  });

  it("tracks connection count", () => {
    const logger = new DebugLogger();
    logger.recordConnection();
    logger.recordConnection();
    expect(logger.getMetrics().connectionCount).toBe(2);
  });
});
