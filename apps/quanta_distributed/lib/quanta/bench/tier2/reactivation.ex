defmodule Quanta.Bench.Tier2.Reactivation do
  @moduledoc """
  B2.5 — Reactivation from snapshot benchmark.

  Measures cold start cost when an actor must restore persisted state
  before handling its first message.
  """

  alias Quanta.Bench.Base

  @spec run :: :ok
  def run do
    Base.run("tier2_reactivation", scenarios(), warmup: 2, time: 5)
  end

  defp scenarios do
    %{
      "reactivation_small_state" => fn -> reactivate(1) end,
      "reactivation_medium_state" => fn -> reactivate(100) end,
      "reactivation_large_state" => fn -> reactivate(10_000) end
    }
  end

  defp reactivate(state_size) do
    snapshot = Map.new(1..state_size, fn i -> {i, i} end)
    parent = self()
    ref = make_ref()

    spawn_link(fn ->
      # Simulate snapshot restore
      count = map_size(snapshot)

      receive do
        {:cmd, :inc, from, r} ->
          send(from, {:ok, r, count + 1})
      end
    end)
    |> then(fn pid -> send(pid, {:cmd, :inc, parent, ref}) end)

    receive do
      {:ok, ^ref, _} -> :ok
    after
      5_000 -> raise "Reactivation timed out"
    end
  end
end
