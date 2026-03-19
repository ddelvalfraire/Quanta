defmodule Quanta.Bench.Tier2.WarmMessage do
  @moduledoc """
  B2.2 — Warm message delivery benchmark.

  Pre-activates an actor and then measures the cost of sending messages to it
  through a registry lookup + send + reply cycle.

  SLO: p99 < 1 ms.
  """

  alias Quanta.Bench.Base

  @spec run :: :ok
  def run do
    Base.run("tier2_warm_message", scenarios(), warmup: 2, time: 5)
  end

  defp scenarios do
    registry = :ets.new(:bench_warm_reg, [:set, :public])
    actor_id = {:bench, :counter, 0}

    pid =
      spawn(fn ->
        counter_loop(0)
      end)

    :ets.insert(registry, {actor_id, pid})

    scenarios = %{
      "warm_single_message" => fn ->
        [{^actor_id, actor}] = :ets.lookup(registry, actor_id)
        ref = make_ref()
        send(actor, {:cmd, :inc, self(), ref})

        receive do
          {:ok, ^ref, _} -> :ok
        after
          5_000 -> raise "Warm message timed out"
        end
      end,
      "warm_burst_100" => fn ->
        [{^actor_id, actor}] = :ets.lookup(registry, actor_id)

        refs =
          for _ <- 1..100 do
            ref = make_ref()
            send(actor, {:cmd, :inc, self(), ref})
            ref
          end

        for ref <- refs do
          receive do
            {:ok, ^ref, _} -> :ok
          after
            5_000 -> raise "Warm burst timed out"
          end
        end
      end
    }

    # Return scenarios; the actor stays alive for the entire benchmark run.
    # Benchee will call these functions many times during warmup + timed runs.
    scenarios
  end

  defp counter_loop(state) do
    receive do
      {:cmd, :inc, from, ref} ->
        new_state = state + 1
        send(from, {:ok, ref, new_state})
        counter_loop(new_state)

      :stop ->
        :ok
    end
  end
end
