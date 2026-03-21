defmodule Quanta.Bench.Tier5 do
  @moduledoc """
  Tier 5: NIF Delta Encoding benchmarks.

  B5.1 -- Schema compilation, delta compute/apply, state decode via NIF
  B5.2 -- Concurrent delta throughput at varying parallelism levels
  """

  @doc "Run all Tier 5 benchmarks."
  @spec run_all :: :ok
  def run_all do
    modules()
    |> Enum.each(fn mod ->
      IO.puts("\n=== #{inspect(mod)} ===\n")
      mod.run()
    end)
  end

  @doc "Returns all Tier 5 benchmark modules."
  @spec modules :: [module()]
  def modules do
    [
      Quanta.Bench.Tier5.NifDelta
    ]
  end
end
