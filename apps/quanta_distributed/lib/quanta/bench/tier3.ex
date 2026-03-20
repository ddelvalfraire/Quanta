defmodule Quanta.Bench.Tier3 do
  @moduledoc """
  Tier 3: CRDT performance benchmarks.

  B3.1 -- 260K sequential inserts from trace
  B3.2 -- Snapshot export/import roundtrip
  B3.3 -- N concurrent editors on same doc
  B3.4 -- Real editing trace replay
  B3.5 -- Cross-replica delta merge
  """

  @doc "Run all Tier 3 benchmarks."
  @spec run_all :: :ok
  def run_all do
    modules()
    |> Enum.each(fn mod ->
      IO.puts("\n=== #{inspect(mod)} ===\n")
      mod.run()
    end)
  end

  @doc "Returns all Tier 3 benchmark modules."
  @spec modules :: [module()]
  def modules do
    [
      Quanta.Bench.Tier3.B1Trace,
      Quanta.Bench.Tier3.SnapshotRoundtrip,
      Quanta.Bench.Tier3.ConcurrentEditing,
      Quanta.Bench.Tier3.B4Trace,
      Quanta.Bench.Tier3.DeltaMerge
    ]
  end
end
