defmodule Quanta.Bench.Tier2 do
  @moduledoc """
  Tier 2: Virtual actor lifecycle benchmarks.

  B2.1 — Cold activation
  B2.2 — Warm message delivery
  B2.3 — Activation storm
  B2.4 — Passivation
  B2.5 — Reactivation from snapshot
  """

  @doc "Run all Tier 2 benchmarks."
  @spec run_all :: :ok
  def run_all do
    modules()
    |> Enum.each(fn mod ->
      IO.puts("\n=== #{inspect(mod)} ===\n")
      mod.run()
    end)
  end

  @doc "Returns all Tier 2 benchmark modules."
  @spec modules :: [module()]
  def modules do
    [
      Quanta.Bench.Tier2.ColdActivation,
      Quanta.Bench.Tier2.WarmMessage,
      Quanta.Bench.Tier2.ActivationStorm,
      Quanta.Bench.Tier2.Passivation,
      Quanta.Bench.Tier2.Reactivation
    ]
  end
end
