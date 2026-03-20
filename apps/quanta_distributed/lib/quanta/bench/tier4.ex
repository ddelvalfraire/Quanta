defmodule Quanta.Bench.Tier4 do
  @moduledoc """
  Tier 4: Differentiator benchmarks.

  B4.1 -- Mixed read/write workload
  B4.2 -- WASM function call overhead
  B4.3 -- 500K actor steady state
  B4.4 -- CRDT vs OT comparison
  B4.5 -- Simulated multi-region latency
  """

  @doc "Run all Tier 4 benchmarks."
  @spec run_all :: :ok
  def run_all do
    modules()
    |> Enum.each(fn mod ->
      IO.puts("\n=== #{inspect(mod)} ===\n")
      mod.run()
    end)
  end

  @doc "Returns all Tier 4 benchmark modules."
  @spec modules :: [module()]
  def modules do
    [
      Quanta.Bench.Tier4.MixedRW,
      Quanta.Bench.Tier4.WasmOverhead,
      Quanta.Bench.Tier4.SteadyState500k,
      Quanta.Bench.Tier4.CrdtVsOt,
      Quanta.Bench.Tier4.MultiRegionSim
    ]
  end
end
