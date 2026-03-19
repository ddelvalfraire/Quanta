defmodule Quanta.Bench.Tier4.SteadyState500k do
  @moduledoc "B4.3 -- Large actor population. Measures spawn + message throughput at scale."

  alias Quanta.Bench.Base

  @spec run :: :ok
  def run do
    Base.run("tier4_steady_state", %{
      "spawn_10k" => fn ->
        pids = for _ <- 1..10_000, do: spawn_link(fn -> receive do: (:ping -> :ok) end)
        for pid <- pids, do: send(pid, :ping)
      end,
      "spawn_100k" => fn ->
        pids = for _ <- 1..100_000, do: spawn_link(fn -> receive do: (:ping -> :ok) end)
        for pid <- pids, do: send(pid, :ping)
      end,
      "message_10k_registered" => fn ->
        # Spawn actors, register in ETS, then round-robin message them
        reg = :ets.new(:bench_ss, [:set, :public])

        _pids =
          for i <- 1..10_000 do
            pid =
              spawn_link(fn ->
                receive do
                  {:ping, from, ref} -> send(from, {:pong, ref})
                end
              end)

            :ets.insert(reg, {i, pid})
            pid
          end

        parent = self()

        refs =
          for i <- 1..10_000 do
            [{_, pid}] = :ets.lookup(reg, i)
            ref = make_ref()
            send(pid, {:ping, parent, ref})
            ref
          end

        for ref <- refs do
          receive do
            {:pong, ^ref} -> :ok
          after
            10_000 -> raise "steady_state timed out"
          end
        end

        :ets.delete(reg)
      end
    }, warmup: 1, time: 5)
  end
end
