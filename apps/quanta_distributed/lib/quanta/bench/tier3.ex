defmodule Quanta.Bench.Tier3 do
  @moduledoc "Tier 3: CRDT performance benchmarks. Populated by PR 17."

  @spec run_all :: :ok
  def run_all do
    modules() |> Enum.each(fn mod -> mod.run() end)
  end

  @spec modules :: [module()]
  def modules, do: []
end
