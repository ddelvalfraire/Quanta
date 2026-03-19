defmodule Quanta.Bench.Tier2.ColdActivation do
  @moduledoc """
  B2.1 — Cold activation benchmark.

  Simulates activating an actor that isn't in the registry: registry lookup
  (miss), spawn, register, and deliver the first message. Each iteration uses
  a unique ID so the actor is always "cold".

  SLO: p99 < 50 ms.
  """

  alias Quanta.Bench.Base

  @spec run :: :ok
  def run do
    Base.run("tier2_cold_activation", scenarios(), warmup: 2, time: 5)
  end

  defp scenarios do
    %{
      "cold_activation" => fn ->
        cold_activate_and_inc()
      end
    }
  end

  # Simulates: registry miss → spawn actor → register → deliver "inc".
  defp cold_activate_and_inc do
    actor_id = {:bench, :counter, System.unique_integer([:positive])}
    parent = self()
    ref = make_ref()

    # Registry lookup (ETS miss)
    registry = :ets.new(:bench_reg, [:set, :public])
    [] = :ets.lookup(registry, actor_id)

    # Activation: spawn + register
    pid =
      spawn(fn ->
        :ets.insert(registry, {actor_id, self()})
        counter_loop(0, actor_id, registry)
      end)

    # Deliver "inc" command
    send(pid, {:cmd, :inc, parent, ref})

    receive do
      {:ok, ^ref, 1} -> :ok
    after
      5_000 -> raise "Cold activation timed out"
    end

    :ets.delete(registry)
  end

  defp counter_loop(state, actor_id, registry) do
    receive do
      {:cmd, :inc, from, ref} ->
        new_state = state + 1
        send(from, {:ok, ref, new_state})
        counter_loop(new_state, actor_id, registry)

      :stop ->
        :ets.delete(registry, actor_id)
    end
  end
end
