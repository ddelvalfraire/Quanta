defmodule Quanta.Bench.Tier2.Reactivation do
  @moduledoc """
  B2.5 — Reactivation from snapshot benchmark.

  Measures cold start cost when an actor must restore persisted state before
  handling its first message. State is stored in ETS (simulating a snapshot
  store) and loaded during activation.
  """

  alias Quanta.Bench.Base

  @spec run :: :ok
  def run do
    Base.run("tier2_reactivation", scenarios(), warmup: 2, time: 5)
  end

  defp scenarios do
    snapshot_store = :ets.new(:bench_snapshots, [:set, :public])

    %{
      "reactivation_small_state" => fn ->
        reactivate(snapshot_store, _state_size = 1)
      end,
      "reactivation_medium_state" => fn ->
        reactivate(snapshot_store, _state_size = 100)
      end,
      "reactivation_large_state" => fn ->
        reactivate(snapshot_store, _state_size = 10_000)
      end
    }
  end

  defp reactivate(snapshot_store, state_size) do
    actor_id = {:bench, :counter, System.unique_integer([:positive])}
    registry = :ets.new(:bench_react_reg, [:set, :public])
    parent = self()

    # Pre-persist a snapshot (simulate prior passivation)
    snapshot = build_snapshot(state_size)
    :ets.insert(snapshot_store, {{:snapshot, actor_id}, snapshot})

    # Cold activation with snapshot restore
    ref = make_ref()

    pid =
      spawn(fn ->
        # Load snapshot (simulates reading from persistence)
        [{_, persisted_state}] = :ets.lookup(snapshot_store, {:snapshot, actor_id})
        initial_count = map_size(persisted_state)

        :ets.insert(registry, {actor_id, self()})
        reactivated_loop(initial_count, actor_id, registry)
      end)

    send(pid, {:cmd, :inc, parent, ref})

    receive do
      {:ok, ^ref, _count} -> :ok
    after
      5_000 -> raise "Reactivation timed out"
    end

    # Cleanup
    :ets.delete(snapshot_store, {:snapshot, actor_id})
    send(pid, :stop)
    :ets.delete(registry)
  end

  # Builds a map of `size` entries to simulate persisted actor state.
  defp build_snapshot(size) do
    Map.new(1..size, fn i -> {i, i} end)
  end

  defp reactivated_loop(state, actor_id, registry) do
    receive do
      {:cmd, :inc, from, ref} ->
        new_state = state + 1
        send(from, {:ok, ref, new_state})
        reactivated_loop(new_state, actor_id, registry)

      :stop ->
        :ets.delete(registry, actor_id)
    end
  end
end
