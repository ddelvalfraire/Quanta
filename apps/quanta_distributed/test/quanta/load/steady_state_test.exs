defmodule Quanta.Load.SteadyStateTest do
  @moduledoc """
  LD1: Steady-state load test.

  Registers 500K actors in the Syn registry and measures lookup latency under
  sustained load. The primary SLO is Syn lookup p99 < 1 ms.
  """

  use ExUnit.Case, async: false

  @moduletag :load
  @moduletag timeout: 600_000

  @actor_count 500_000

  # SLO: Syn lookup p99 < 1 ms with 500K registered actors
  # SLO: memory per actor < 2 KB average

  test "500K actors registered with p99 lookup under SLO" do
    # TODO: Register @actor_count actors in Syn with unique ActorId keys
    # TODO: Perform 10K random lookups, record latency for each
    # TODO: Compute p50, p95, p99 from latency samples
    # TODO: Assert p99 < 1_000 (microseconds)
    # TODO: Assert total memory / @actor_count < 2048 (bytes per actor)
  end

  test "lookup latency remains stable under continuous churn" do
    # TODO: Register 500K actors
    # TODO: Spawn a background task that continuously deregisters + re-registers
    #       actors at ~1K/sec churn rate
    # TODO: Concurrently perform 10K lookups, record latencies
    # TODO: Assert p99 < 2_000 (microseconds) — relaxed SLO under churn
  end
end
