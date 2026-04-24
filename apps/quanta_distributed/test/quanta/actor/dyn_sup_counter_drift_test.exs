defmodule Quanta.Actor.DynSup.CounterDriftTest do
  use ExUnit.Case, async: false

  # HIGH-2: DynSup.track_actor/1 spawns an unsupervised monitor process via
  # bare spawn/1 (dyn_sup.ex:81).  If that process is killed before it
  # receives {:DOWN, ...} from the actor, :atomics.sub is never called and the
  # fast counter permanently overcounts.
  #
  # Reproduction steps:
  #   1. Snapshot Process.list() before start_actor.
  #   2. Call start_actor; the anonymous monitor is the new pid that is neither
  #      the actor pid nor a pre-existing process.
  #   3. Kill the monitor with :kill BEFORE stopping the actor.
  #   4. Stop the actor normally (so the supervisor-tree count is correct).
  #   5. Assert count_actors_fast() == 0.
  #      FAILS today: returns 1 because :atomics.sub was never executed.

  alias Quanta.Actor.DynSup
  alias Quanta.ActorId

  defp temp_agent_spec(fun \\ fn -> nil end) do
    Map.put(Agent.child_spec(fun), :restart, :temporary)
  end

  defp make_actor_id(id) do
    %ActorId{namespace: "test", type: "counter", id: id}
  end

  # Returns pids that appeared in Process.list() after the snapshot, minus the
  # known actor pid and the current test pid.
  defp new_pids_since(snapshot_set, exclude_pid) do
    MapSet.new(Process.list())
    |> MapSet.difference(snapshot_set)
    |> MapSet.delete(exclude_pid)
    |> MapSet.delete(self())
    |> MapSet.to_list()
  end

  setup do
    PartitionSupervisor.which_children(DynSup)
    |> Enum.each(fn {_id, sup_pid, _type, _modules} ->
      DynamicSupervisor.which_children(sup_pid)
      |> Enum.each(fn {_, child_pid, _, _} ->
        DynamicSupervisor.terminate_child(sup_pid, child_pid)
      end)
    end)

    # Let any in-flight :DOWN messages from the cleanup settle so that any
    # live monitor processes that survived a prior test can decrement the
    # atomic counter before we read it.
    Process.sleep(30)

    # Force-reset the atomic counter to 0 so inter-test drift from a
    # previous run does not pollute the sanity checks in this test.
    # DynSup.decrement_count/0 wraps :atomics.sub, so instead we use the
    # public accurate count as the ground truth and reset via :atomics directly.
    ref = :persistent_term.get(:quanta_actor_counter)
    :atomics.put(ref, 1, 0)

    :ok
  end

  describe "HIGH-2: counter drift when monitor process is killed before actor exits" do
    test "count_actors_fast/0 returns 0 after start + monitor-kill + stop — currently drifts to 1" do
      actor_id = make_actor_id("drift-single-#{System.unique_integer([:positive])}")

      # Snapshot before starting the actor so we can find the monitor pid.
      pids_before = MapSet.new(Process.list())

      {:ok, actor_pid} =
        DynSup.start_actor(actor_id, child_spec: temp_agent_spec(fn -> :hello end))

      # Yield to let the spawned monitor process reach its receive block.
      Process.sleep(10)

      candidates = new_pids_since(pids_before, actor_pid)

      assert length(candidates) >= 1,
             "Expected at least one new process (the anonymous monitor); " <>
               "got none. New pids: #{inspect(candidates)}"

      monitor_pid = hd(candidates)

      assert Process.alive?(monitor_pid),
             "Monitor process must be alive before we kill it"

      # Counter is 1 right now (actor is alive, monitor is alive).
      assert DynSup.count_actors_fast() == 1

      # Kill the monitor BEFORE the actor exits — simulates OOM/VM panic.
      Process.exit(monitor_pid, :kill)
      Process.sleep(10)

      refute Process.alive?(monitor_pid), "Monitor should be dead after :kill"

      # Stop the actor cleanly; supervisor-tree count drops to 0.
      actor_ref = Process.monitor(actor_pid)
      DynSup.stop_actor(actor_pid)
      assert_receive {:DOWN, ^actor_ref, :process, ^actor_pid, :normal}, 2_000

      # The accurate traversal agrees: 0 actors.
      assert DynSup.count_actors() == 0,
             "count_actors/0 (accurate) should be 0 after stop"

      # BUG: count_actors_fast/0 should also be 0 but the monitor was killed
      # before it could call :atomics.sub, so it remains at 1.
      # This is the failing assertion that proves HIGH-2.
      assert DynSup.count_actors_fast() == 0,
             "count_actors_fast/0 drifted: expected 0, got #{DynSup.count_actors_fast()}"
    end

    test "drift accumulates: N actors with killed monitors leaves fast counter at N, not 0" do
      n = 3

      # Start N actors, capturing each monitor pid.
      actor_monitor_pairs =
        Enum.map(1..n, fn i ->
          actor_id = make_actor_id("drift-multi-#{i}-#{System.unique_integer([:positive])}")
          snapshot = MapSet.new(Process.list())

          {:ok, actor_pid} =
            DynSup.start_actor(actor_id, child_spec: temp_agent_spec(fn -> i end))

          Process.sleep(10)
          candidates = new_pids_since(snapshot, actor_pid)
          {actor_pid, candidates}
        end)

      # Sanity: fast counter reflects all N actors.
      assert DynSup.count_actors_fast() == n

      # Kill every monitor process before any actor exits.
      for {_actor_pid, monitor_pids} <- actor_monitor_pairs do
        Enum.each(monitor_pids, fn pid ->
          if Process.alive?(pid), do: Process.exit(pid, :kill)
        end)
      end

      Process.sleep(20)

      # Now stop all actors cleanly.
      for {actor_pid, _monitors} <- actor_monitor_pairs do
        ref = Process.monitor(actor_pid)

        try do
          DynSup.stop_actor(actor_pid)
        catch
          :exit, _ -> :ok
        end

        assert_receive {:DOWN, ^ref, :process, ^actor_pid, _reason}, 2_000
      end

      # Supervisor-tree count is correct: 0.
      assert DynSup.count_actors() == 0,
             "count_actors/0 (accurate) should be 0 after all actors stopped"

      # BUG: fast counter drifts — stays at n because no monitor ran :atomics.sub.
      assert DynSup.count_actors_fast() == 0,
             "count_actors_fast/0 drifted: expected 0, got #{DynSup.count_actors_fast()}"
    end
  end
end
