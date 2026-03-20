defmodule Quanta.Bench.Tier2.ActivationStorm do
  @moduledoc """
  B2.3 — Activation storm benchmark.

  Activates N actors in rapid succession, each spawned, sent one message,
  and confirmed. All actors exit after responding.
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
    parent = self()

    refs =
      for _ <- 1..n do
        ref = make_ref()

        spawn_link(fn ->
          receive do
            {:cmd, :inc, from, r} -> send(from, {:ok, r, 1})
          end
        end)
        |> then(fn pid -> send(pid, {:cmd, :inc, parent, ref}); ref end)
      end

    for ref <- refs do
      receive do
        {:ok, ^ref, 1} -> :ok
      after
        30_000 -> raise "Storm timed out"
      end
    end
  end
end
