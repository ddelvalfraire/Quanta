defmodule Quanta.Bench.Tier2.ActivationStorm do
  @moduledoc """
  B2.3 — Activation storm benchmark.

  Activates many actors in rapid succession, simulating a burst of cold
  activations (e.g. after a node restart or rebalance). Each actor is
  spawned, registered, sent one message, and confirmed.

  SLO: p99 < 100 ms.
  """

  alias Quanta.Bench.Base

  @spec run :: :ok
  def run do
    Base.run("tier2_activation_storm", scenarios(), warmup: 1, time: 5)
  end

  defp scenarios do
    %{
      "storm_10" => fn -> run_storm(10) end,
      "storm_100" => fn -> run_storm(100) end,
      "storm_1000" => fn -> run_storm(1_000) end
    }
  end

  defp run_storm(n) do
    registry = :ets.new(:bench_storm_reg, [:set, :public])
    parent = self()
    base = System.unique_integer([:positive])

    refs =
      for i <- 1..n do
        actor_id = {:bench, :counter, base + i}
        ref = make_ref()

        pid =
          spawn(fn ->
            :ets.insert(registry, {actor_id, self()})
            storm_actor(actor_id, registry)
          end)

        send(pid, {:cmd, :inc, parent, ref})
        ref
      end

    for ref <- refs do
      receive do
        {:ok, ^ref, 1} -> :ok
      after
        30_000 -> raise "Activation storm timed out"
      end
    end

    :ets.delete(registry)
  end

  defp storm_actor(actor_id, registry) do
    receive do
      {:cmd, :inc, from, ref} ->
        send(from, {:ok, ref, 1})

      :stop ->
        :ets.delete(registry, actor_id)
    end
  end
end
