defmodule Quanta.SupervisorTest do
  use ExUnit.Case, async: false

  describe "supervision tree" do
    test "all expected children are alive" do
      children =
        Quanta.Supervisor
        |> Supervisor.which_children()
        |> Map.new(fn {id, pid, _type, _modules} -> {id, pid} end)

      assert is_pid(children[Quanta.HLC.Server])
      assert Process.alive?(children[Quanta.HLC.Server])

      assert is_pid(children[Quanta.Wasm.EngineManager])
      assert Process.alive?(children[Quanta.Wasm.EngineManager])

      assert is_pid(children[Quanta.Wasm.ModuleRegistry])
      assert Process.alive?(children[Quanta.Wasm.ModuleRegistry])

      assert is_pid(children[Quanta.Actor.ManifestRegistry])
      assert Process.alive?(children[Quanta.Actor.ManifestRegistry])

      assert is_pid(children[Quanta.Actor.DynSup])
      assert Process.alive?(children[Quanta.Actor.DynSup])

      assert is_pid(children[Quanta.SideEffect.TaskSupervisor])
      assert Process.alive?(children[Quanta.SideEffect.TaskSupervisor])

      assert is_pid(children[Quanta.Actor.CompactionScheduler])
      assert Process.alive?(children[Quanta.Actor.CompactionScheduler])
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
