defmodule Quanta.Load.MixedWorkloadTest do
  @moduledoc """
  LD5: Mixed read/write workload load test.

  Simulates 10K concurrent connections with a 75/25 read/write split,
  representative of a realistic production workload.
  """

  use ExUnit.Case, async: false

  @moduletag :load
  @moduletag timeout: 600_000

  @connection_count 10_000
  @read_ratio 0.75
  @write_ratio 0.25
  @ops_per_connection 100

  # SLO: read p99 < 5 ms
  # SLO: write p99 < 20 ms
  # SLO: zero errors under sustained mixed load
  # SLO: system remains responsive (no starvation of reads by writes)

  test "10K connections with 75/25 read/write split" do
    # TODO: Pre-populate a pool of actors (e.g., 1K actors with initial state)
    # TODO: Spawn @connection_count tasks, each performing @ops_per_connection ops
    #       - 75% reads (lookup actor, get state)
    #       - 25% writes (send command, await response)
    # TODO: Record per-op latency, bucketed by read vs write
    # TODO: Compute read p50, p95, p99 and write p50, p95, p99
    # TODO: Assert read p99 < 5_000 (microseconds)
    # TODO: Assert write p99 < 20_000 (microseconds)
    # TODO: Assert zero errors
  end

  test "mixed workload does not starve reads under write pressure" do
    # TODO: Same setup but shift to 50/50 read/write to increase write pressure
    # TODO: Assert read p99 does not degrade more than 2x vs the 75/25 baseline
    # TODO: Assert zero errors
  end
end
