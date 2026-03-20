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
    parent = self()

    pid =
      spawn(fn ->
        passivating_actor(0, @idle_timeout_ms)
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
      {:DOWN, _mref, :process, ^pid, _reason} -> :ok
    after
      5_000 -> raise "Passivation timed out"
    end
  end

  defp passivate_batch(n) do
    parent = self()

    pids =
      for _ <- 1..n do
        pid =
          spawn(fn ->
            passivating_actor(0, @idle_timeout_ms)
          end)

        Process.monitor(pid)
        ref = make_ref()
        send(pid, {:cmd, :inc, parent, ref})

        receive do
          {:ok, ^ref, 1} -> :ok
        after
          5_000 -> raise "Batch passivation setup timed out"
        end

        pid
      end

    for pid <- pids do
      receive do
        {:DOWN, _mref, :process, ^pid, _reason} -> :ok
      after
        5_000 -> raise "Batch passivation timed out"
      end
    end
  end

  defp passivating_actor(state, idle_timeout) do
    receive do
      {:cmd, :inc, from, ref} ->
        send(from, {:ok, ref, state + 1})
        passivating_actor(state + 1, idle_timeout)
    after
      idle_timeout ->
        exit(:passivated)
    end
  end
end
