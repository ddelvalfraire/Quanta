defmodule Quanta.Bench.Tier1 do
  @moduledoc """
  Tier 1: Core actor runtime benchmarks.

  B1.1 — Ping-pong (local + remote)
  B1.2 — Fan-out broadcast (1→N, N=10,100,1000)
  B1.3 — (reserved for future)
  B1.4 — Skynet 1M actor tree
  B1.5 — Concurrent mailbox contention (N→1)
  """

  @doc "Run all Tier 1 benchmarks."
  @spec run_all :: :ok
  def run_all do
    modules()
    |> Enum.each(fn mod ->
      IO.puts("\n=== #{inspect(mod)} ===\n")
      mod.run()
    end)
  end

  @doc "Returns all Tier 1 benchmark modules."
  @spec modules :: [module()]
  def modules do
    [
      Quanta.Bench.Tier1.PingPong,
      Quanta.Bench.Tier1.FanOut,
      Quanta.Bench.Tier1.Skynet,
      Quanta.Bench.Tier1.ConcurrentMailbox
    ]
  end
end
