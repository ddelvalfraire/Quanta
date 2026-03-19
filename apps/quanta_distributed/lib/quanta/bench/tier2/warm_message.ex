defmodule Quanta.Bench.Tier2.WarmMessage do
  @moduledoc """
  B2.2 — Warm message delivery benchmark.

  Pre-activates a long-lived actor and measures send + reply cost.
  SLO: p99 < 1 ms.
  """

  alias Quanta.Bench.Base

  @spec run :: :ok
  def run do
    Base.run("tier2_warm_message", scenarios(), warmup: 2, time: 5)
  end

  defp scenarios do
    actor = spawn_link(fn -> counter_loop(0) end)

    %{
      "warm_single_message" => fn ->
        ref = make_ref()
        send(actor, {:cmd, :inc, self(), ref})

        receive do
          {:ok, ^ref, _} -> :ok
        after
          5_000 -> raise "Warm message timed out"
        end
      end,
      "warm_burst_100" => fn ->
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
  end

  defp counter_loop(state) do
    receive do
      {:cmd, :inc, from, ref} ->
        send(from, {:ok, ref, state + 1})
        counter_loop(state + 1)
    end
  end
end
