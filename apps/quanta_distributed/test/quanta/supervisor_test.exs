defmodule Quanta.SupervisorTest do
  use ExUnit.Case, async: false

  # After HIGH-3, the supervision tree is split into two sub-supervisors
  # under Quanta.Supervisor. Children retain their registered names, so
  # lookup via `Process.whereis/1` (and therefore every caller that uses
  # the name) still works across the restructure.

  defp alive_by_name?(name) do
    pid = Process.whereis(name)
    is_pid(pid) and Process.alive?(pid)
  end

  describe "supervision tree" do
    test "all expected children are alive (across sub-supervisors)" do
      # Infrastructure layer
      assert alive_by_name?(Quanta.Infrastructure.Supervisor)
      assert alive_by_name?(Quanta.SynConfig)
      assert alive_by_name?(Quanta.ClusterSupervisor)
      assert alive_by_name?(Quanta.Cluster.Topology)
      assert alive_by_name?(Quanta.HLC.Server)
      assert alive_by_name?(Quanta.Wasm.EngineManager)
      assert alive_by_name?(Quanta.Wasm.ModuleRegistry)
      assert alive_by_name?(Quanta.Actor.ManifestRegistry)
      assert alive_by_name?(Quanta.SideEffect.TaskSupervisor)
      assert alive_by_name?(Quanta.Nats.CoreSupervisor)
      assert alive_by_name?(Quanta.Nats.JetStream.Connection)
      assert alive_by_name?(Quanta.Broadway.PipelineSupervisor)

      # Actor layer
      assert alive_by_name?(Quanta.Actor.Supervisor)
      assert alive_by_name?(Quanta.Actor.DynSup)
      assert alive_by_name?(Quanta.Actor.CommandRouter)
      assert alive_by_name?(Quanta.Actor.CompactionScheduler)
    end

    test "DynSup partitions have max_restarts 10_000" do
      PartitionSupervisor.which_children(Quanta.Actor.DynSup)
      |> Enum.each(fn {_id, pid, _type, _modules} ->
        state = :sys.get_state(pid)
        assert state.max_restarts == 10_000
        assert state.max_seconds == 1
      end)
    end

    test "TaskSupervisor is alive and named" do
      pid = Process.whereis(Quanta.SideEffect.TaskSupervisor)
      assert is_pid(pid)
      assert Process.alive?(pid)
    end
  end
end
