defmodule Quanta.Bench.Tier2.Passivation do
  @moduledoc """
  B2.4 — Passivation benchmark.

  Measures the time for an actor to detect idle timeout and passivate
  (persist state + deregister + exit). Uses a short timeout to keep
  iterations fast.
  """

  alias Quanta.Bench.Base

  # Short idle timeout for benchmarking (milliseconds).
  @idle_timeout_ms 5

  @spec run :: :ok
  def run do
    Base.run("tier2_passivation", scenarios(), warmup: 2, time: 5)
  end

  defp scenarios do
    %{
      "passivation_single" => fn ->
        passivate_one()
      end,
      "passivation_batch_50" => fn ->
        passivate_batch(50)
      end
    }
  end

  defp passivate_one do
    registry = :ets.new(:bench_pass_reg, [:set, :public])
    actor_id = {:bench, :counter, System.unique_integer([:positive])}
    parent = self()

    pid =
      spawn(fn ->
        :ets.insert(registry, {actor_id, self()})
        passivating_actor(0, actor_id, registry, @idle_timeout_ms)
      end)

    # Monitor so we know when passivation completes
    Process.monitor(pid)

    # Send one message to give the actor state worth "persisting"
    ref = make_ref()
    send(pid, {:cmd, :inc, parent, ref})

    receive do
      {:ok, ^ref, 1} -> :ok
    after
      5_000 -> raise "Passivation setup timed out"
    end

    # Now wait for the actor to passivate (idle timeout → exit)
    receive do
      {:DOWN, _mref, :process, ^pid, :passivated} -> :ok
      {:DOWN, _mref, :process, ^pid, _reason} -> :ok
    after
      5_000 -> raise "Passivation timed out"
    end

    :ets.delete(registry)
  end

  defp passivate_batch(n) do
    registry = :ets.new(:bench_pass_batch_reg, [:set, :public])
    parent = self()
    base = System.unique_integer([:positive])

    pids =
      for i <- 1..n do
        actor_id = {:bench, :counter, base + i}

        pid =
          spawn(fn ->
            :ets.insert(registry, {actor_id, self()})
            passivating_actor(0, actor_id, registry, @idle_timeout_ms)
          end)

        Process.monitor(pid)

        # Send one message so actor has state
        ref = make_ref()
        send(pid, {:cmd, :inc, parent, ref})

        receive do
          {:ok, ^ref, 1} -> :ok
        after
          5_000 -> raise "Batch passivation setup timed out"
        end

        pid
      end

    # Wait for all actors to passivate
    for pid <- pids do
      receive do
        {:DOWN, _mref, :process, ^pid, _reason} -> :ok
      after
        5_000 -> raise "Batch passivation timed out"
      end
    end

    :ets.delete(registry)
  end

  defp passivating_actor(state, actor_id, registry, idle_timeout) do
    receive do
      {:cmd, :inc, from, ref} ->
        new_state = state + 1
        send(from, {:ok, ref, new_state})
        passivating_actor(new_state, actor_id, registry, idle_timeout)

      :stop ->
        :ets.delete(registry, actor_id)
    after
      idle_timeout ->
        # Simulate snapshot persistence (write state to ETS)
        :ets.insert(registry, {{:snapshot, actor_id}, state})
        :ets.delete(registry, actor_id)
        exit(:passivated)
    end
  end
end
