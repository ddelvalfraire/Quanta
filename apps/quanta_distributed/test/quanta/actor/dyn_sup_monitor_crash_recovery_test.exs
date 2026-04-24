defmodule Quanta.Actor.DynSup.MonitorCrashRecoveryTest do
  use ExUnit.Case, async: false

  # CRITICAL-1: When Quanta.Actor.DynSup.Monitor crashes, its internal
  # :one_for_one supervisor restarts ONLY the Monitor — DynSup.start_link
  # is NOT re-entered, so the atomic counter is not reset. Monitor's init/1
  # starts with an empty ref map, so all previously-tracked actor pids are
  # un-monitored. Their future exits never decrement the counter, and
  # count_actors_fast/0 inflates permanently.
  #
  # This test drives the failure: start N actors, kill the Monitor, then
  # cleanly stop the actors. The fast counter must return to 0.

  alias Quanta.Actor.DynSup
  alias Quanta.ActorId

  defp temp_agent_spec(fun \\ fn -> nil end) do
    Map.put(Agent.child_spec(fun), :restart, :temporary)
  end

  defp make_actor_id(id) do
    %ActorId{namespace: "test", type: "monitor_crash", id: id}
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

  defp wait_until(fun, expected, timeout_ms \\ 2_000) do
    deadline = System.monotonic_time(:millisecond) + timeout_ms

    Stream.repeatedly(fn ->
      Process.sleep(10)
      fun.()
    end)
    |> Enum.find(fn value ->
      value == expected or System.monotonic_time(:millisecond) > deadline
    end)
  end

  defp wait_until_restarted(old_pid, timeout_ms) do
    deadline = System.monotonic_time(:millisecond) + timeout_ms

    Stream.repeatedly(fn ->
      Process.sleep(10)
      Process.whereis(Quanta.Actor.DynSup.Monitor)
    end)
    |> Enum.find(fn
      pid when is_pid(pid) and pid != old_pid -> true
      _ -> System.monotonic_time(:millisecond) > deadline
    end)
  end

  test "counter recovers to 0 after Monitor crash and clean actor shutdown" do
    n = 5

    pids =
      for i <- 1..n do
        actor_id = make_actor_id("mc-#{i}-#{System.unique_integer([:positive])}")
        {:ok, pid} = DynSup.start_actor(actor_id, child_spec: temp_agent_spec())
        pid
      end

    assert wait_until(&DynSup.count_actors_fast/0, n) == n
    assert DynSup.count_actors() == n

    monitor_pid = Process.whereis(Quanta.Actor.DynSup.Monitor)
    assert is_pid(monitor_pid)
    monitor_ref = Process.monitor(monitor_pid)
    Process.exit(monitor_pid, :kill)
    assert_receive {:DOWN, ^monitor_ref, :process, ^monitor_pid, :killed}, 1_000

    new_monitor_pid = wait_until_restarted(monitor_pid, 2_000)
    assert is_pid(new_monitor_pid)
    assert new_monitor_pid != monitor_pid

    refs =
      Enum.map(pids, fn pid ->
        ref = Process.monitor(pid)
        DynSup.stop_actor(pid)
        {ref, pid}
      end)

    Enum.each(refs, fn {ref, pid} ->
      assert_receive {:DOWN, ^ref, :process, ^pid, :normal}, 2_000
    end)

    assert wait_until(&DynSup.count_actors_fast/0, 0) == 0,
           "count_actors_fast drifted after Monitor crash: expected 0, got #{DynSup.count_actors_fast()}"

    assert DynSup.count_actors() == 0
  end
end
