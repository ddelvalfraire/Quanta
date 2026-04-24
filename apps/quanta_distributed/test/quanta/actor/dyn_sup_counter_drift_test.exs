defmodule Quanta.Actor.DynSup.CounterDriftTest do
  use ExUnit.Case, async: false

  # HIGH-2: `count_actors_fast/0` (atomic counter) must stay in sync with
  # `count_actors/0` (supervisor-tree walk) across all actor lifecycle paths —
  # clean stop, abrupt kill, and mixed start/stop bursts. The prior
  # implementation used an unsupervised `spawn/1` to decrement the counter on
  # :DOWN; if that process was killed exogenously (OOM, VM panic) before it
  # observed :DOWN, the counter drifted upward indefinitely and eventually
  # starved capacity.

  alias Quanta.Actor.DynSup
  alias Quanta.ActorId

  defp temp_agent_spec(fun \\ fn -> nil end) do
    Map.put(Agent.child_spec(fun), :restart, :temporary)
  end

  defp make_actor_id(id) do
    %ActorId{namespace: "test", type: "counter", id: id}
  end

  setup do
    PartitionSupervisor.which_children(DynSup)
    |> Enum.each(fn {_id, sup_pid, _type, _modules} ->
      DynamicSupervisor.which_children(sup_pid)
      |> Enum.each(fn {_, child_pid, _, _} ->
        DynamicSupervisor.terminate_child(sup_pid, child_pid)
      end)
    end)

    Process.sleep(30)

    ref = :persistent_term.get(:quanta_actor_counter)
    :atomics.put(ref, 1, 0)

    :ok
  end

  defp wait_until_fast_matches(expected, timeout_ms \\ 1_000) do
    deadline = System.monotonic_time(:millisecond) + timeout_ms

    Stream.repeatedly(fn ->
      Process.sleep(10)
      DynSup.count_actors_fast()
    end)
    |> Enum.find(fn n ->
      n == expected or System.monotonic_time(:millisecond) > deadline
    end)
  end

  describe "fast counter stays in sync with supervisor tree" do
    test "count_actors_fast agrees with count_actors after clean starts and stops" do
      n = 5

      pids =
        for i <- 1..n do
          actor_id = make_actor_id("clean-#{i}-#{System.unique_integer([:positive])}")
          {:ok, pid} = DynSup.start_actor(actor_id, child_spec: temp_agent_spec())
          pid
        end

      assert wait_until_fast_matches(n) == n
      assert DynSup.count_actors() == n

      refs =
        Enum.map(pids, fn pid ->
          ref = Process.monitor(pid)
          DynSup.stop_actor(pid)
          {ref, pid}
        end)

      Enum.each(refs, fn {ref, pid} ->
        assert_receive {:DOWN, ^ref, :process, ^pid, :normal}, 2_000
      end)

      assert wait_until_fast_matches(0) == 0
      assert DynSup.count_actors() == 0
    end

    test "counter decrements when actors are killed abruptly (Process.exit :kill)" do
      n = 3

      pids =
        for i <- 1..n do
          actor_id = make_actor_id("kill-#{i}-#{System.unique_integer([:positive])}")
          {:ok, pid} = DynSup.start_actor(actor_id, child_spec: temp_agent_spec())
          pid
        end

      assert wait_until_fast_matches(n) == n

      refs =
        Enum.map(pids, fn pid ->
          ref = Process.monitor(pid)
          Process.exit(pid, :kill)
          {ref, pid}
        end)

      Enum.each(refs, fn {ref, pid} ->
        assert_receive {:DOWN, ^ref, :process, ^pid, :killed}, 2_000
      end)

      assert wait_until_fast_matches(0) == 0,
             "count_actors_fast drifted: expected 0, got #{DynSup.count_actors_fast()}"

      assert DynSup.count_actors() == 0
    end
  end
end
