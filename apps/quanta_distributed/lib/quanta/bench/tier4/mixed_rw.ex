defmodule Quanta.Bench.Tier4.MixedRW do
  @moduledoc "B4.1 -- Mixed R/W workload via ETS-registered actors. SLO: >50K ops/sec."

  alias Quanta.Bench.Base

  @spec run :: :ok
  def run do
    # Clean up a leftover table from a previous run if it exists
    try do
      :ets.delete(:bench_mixed_rw)
    catch
      :error, :badarg -> :ok
    end

    # Pre-create 1K actors registered in ETS
    registry = :ets.new(:bench_mixed_rw, [:set, :public, :named_table])

    actors =
      for i <- 1..1_000 do
        pid =
          spawn_link(fn ->
            actor_loop(%{counter: 0, data: "init"})
          end)

        :ets.insert(registry, {i, pid})
        pid
      end

    try do
      Base.run("tier4_mixed_rw", %{
        "1k_ops_75r_25w" => fn ->
          for i <- 1..1_000 do
            id = :rand.uniform(1_000)
            [{_, pid}] = :ets.lookup(registry, id)
            ref = make_ref()

            if rem(i, 4) == 0 do
              send(pid, {:write, self(), ref, i})
            else
              send(pid, {:read, self(), ref})
            end

            receive do
              {:ok, ^ref, _} -> :ok
            after
              5_000 -> raise "mixed_rw timed out"
            end
          end
        end,
        "1k_ops_50r_50w" => fn ->
          for i <- 1..1_000 do
            id = :rand.uniform(1_000)
            [{_, pid}] = :ets.lookup(registry, id)
            ref = make_ref()

            if rem(i, 2) == 0 do
              send(pid, {:write, self(), ref, i})
            else
              send(pid, {:read, self(), ref})
            end

            receive do
              {:ok, ^ref, _} -> :ok
            after
              5_000 -> raise "mixed_rw timed out"
            end
          end
        end
      }, warmup: 2, time: 10)
    after
      for pid <- actors, do: send(pid, :stop)
      :ets.delete(registry)
    end

    :ok
  end

  defp actor_loop(state) do
    receive do
      {:read, from, ref} ->
        send(from, {:ok, ref, state.counter})
        actor_loop(state)

      {:write, from, ref, val} ->
        send(from, {:ok, ref, val})
        actor_loop(%{state | counter: state.counter + 1, data: "v#{val}"})

      :stop ->
        :ok
    end
  end
end
