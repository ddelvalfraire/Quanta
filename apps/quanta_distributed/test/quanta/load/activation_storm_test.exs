defmodule Quanta.Load.ActivationStormTest do
  @moduledoc """
  LD3: Activation storm load test.

  Activates 10K actors within a 10-second window and verifies the system
  handles the burst without dropping activations or exceeding latency SLOs.
  """

  use ExUnit.Case, async: false

  @moduletag :load
  @moduletag timeout: 600_000

  @actor_count 10_000
  @window_seconds 10

  # SLO: 10K actors activated in <= 10s (1K activations/sec sustained)
  # SLO: zero failed activations
  # SLO: activation p99 < 50 ms

  test "10K actors activate within the time window" do
    # TODO: Generate @actor_count unique ActorIds
    # TODO: Record start time
    # TODO: Send activation messages (e.g., "inc") to all actors concurrently
    #       using Task.async_stream with max_concurrency tuned for throughput
    # TODO: Collect results, count successes and failures
    # TODO: Record end time, compute elapsed
    # TODO: Assert elapsed <= @window_seconds
    # TODO: Assert zero failures
  end

  test "activation latencies stay within SLO during storm" do
    # TODO: Same burst as above but record per-activation latency
    # TODO: Compute p50, p95, p99
    # TODO: Assert p99 < 50_000 (microseconds)
  end
end
