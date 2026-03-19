defmodule Quanta.Bench.Tier2 do
  @moduledoc "Tier 2: Virtual actor lifecycle benchmarks. Populated by PR 16."

  @spec run_all :: :ok
  def run_all do
    modules() |> Enum.each(fn mod -> mod.run() end)
  end

  @spec modules :: [module()]
  def modules, do: []
end
