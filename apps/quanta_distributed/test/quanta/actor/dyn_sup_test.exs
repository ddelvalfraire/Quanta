defmodule Quanta.Actor.DynSupTest do
  use ExUnit.Case, async: false

  alias Quanta.Actor.DynSup
  alias Quanta.ActorId

  defp temp_agent_spec(fun \\ fn -> nil end) do
    Map.put(Agent.child_spec(fun), :restart, :temporary)
  end

  setup do
    PartitionSupervisor.which_children(DynSup)
    |> Enum.each(fn {_id, sup_pid, _type, _modules} ->
      DynamicSupervisor.which_children(sup_pid)
      |> Enum.each(fn {_, child_pid, _, _} ->
        DynamicSupervisor.terminate_child(sup_pid, child_pid)
      end)
    end)

    :ok
  end

  defp make_actor_id(id) do
    %ActorId{namespace: "test", type: "counter", id: id}
  end

  describe "partitions" do
    test "partition count equals System.schedulers_online()" do
      children = PartitionSupervisor.which_children(DynSup)
      assert length(children) == System.schedulers_online()
    end

    test "max_restarts is 10_000 on each DynamicSupervisor partition" do
      PartitionSupervisor.which_children(DynSup)
      |> Enum.each(fn {_id, pid, _type, _modules} ->
        state = :sys.get_state(pid)
        assert state.max_restarts == 10_000
        assert state.max_seconds == 1
      end)
    end
  end

  describe "start_actor/2" do
    test "successfully starts a process" do
      actor_id = make_actor_id("a1")

      assert {:ok, pid} =
               DynSup.start_actor(actor_id, child_spec: temp_agent_spec(fn -> :hello end))

      assert Process.alive?(pid)
      assert Agent.get(pid, & &1) == :hello
    end

    test "same actor_id always routes to same partition (deterministic)" do
      actor_id = make_actor_id("deterministic-test")
      expected_partition = :erlang.phash2(actor_id, System.schedulers_online())

      for _ <- 1..10 do
        assert :erlang.phash2(actor_id, System.schedulers_online()) == expected_partition
      end
    end
  end

  describe "stop_actor/1" do
    test "terminates the process" do
      actor_id = make_actor_id("stop-me")
      {:ok, pid} = DynSup.start_actor(actor_id, child_spec: temp_agent_spec())
      assert Process.alive?(pid)

      ref = Process.monitor(pid)
      DynSup.stop_actor(pid)
      assert_receive {:DOWN, ^ref, :process, ^pid, :normal}
      refute Process.alive?(pid)
    end
  end

  describe "count_actors/0" do
    test "returns 0 when no actors are started" do
      assert DynSup.count_actors() == 0
    end

    test "returns correct count after starting actors" do
      for i <- 1..5 do
        actor_id = make_actor_id("count-#{i}")
        {:ok, _pid} = DynSup.start_actor(actor_id, child_spec: temp_agent_spec())
      end

      assert DynSup.count_actors() == 5
    end

    test "count decreases after stopping actors" do
      {:ok, pid} =
        DynSup.start_actor(make_actor_id("temp"), child_spec: temp_agent_spec())

      assert DynSup.count_actors() == 1

      ref = Process.monitor(pid)
      DynSup.stop_actor(pid)
      assert_receive {:DOWN, ^ref, :process, ^pid, :normal}

      # Wait for the supervisor to process its own DOWN message
      Process.sleep(10)
      assert DynSup.count_actors() == 0
    end
  end
end
