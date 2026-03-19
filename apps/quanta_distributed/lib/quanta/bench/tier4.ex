defmodule Quanta.Bench.Tier4 do
  @moduledoc "Tier 4: Differentiator benchmarks. Populated by PR 18."

  @spec run_all :: :ok
  def run_all do
    modules() |> Enum.each(fn mod -> mod.run() end)
  end

  @spec modules :: [module()]
  def modules, do: []
end
