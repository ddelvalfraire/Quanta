defmodule Quanta.DrainHooksTest do
  use ExUnit.Case, async: false

  alias Quanta.Actor.{CommandRouter, DynSup, Registry}
  alias Quanta.ActorId
  alias Quanta.Cluster.Topology

  defp make_actor_id(id) do
    %ActorId{namespace: "test", type: "counter", id: id}
  end

  defp temp_agent_spec(fun \\ fn -> nil end) do
    Map.put(Agent.child_spec(fun), :restart, :temporary)
  end

  describe "DynSup.list_actor_pids/0" do
    test "returns empty list when no actors are running" do
      assert is_list(DynSup.list_actor_pids())
    end

    test "returns pids of started actors" do
      before = DynSup.list_actor_pids()
      actor_id = make_actor_id("list-pids-1")

      {:ok, pid} = DynSup.start_actor(actor_id, child_spec: temp_agent_spec())

      pids = DynSup.list_actor_pids()
      assert pid in pids
      assert length(pids) == length(before) + 1

      DynSup.stop_actor(pid)
    end
  end

  describe "Topology.remove_self/0" do
    test "removes current node from hash ring and emits telemetry" do
      ref =
        :telemetry_test.attach_event_handlers(self(), [
          [:quanta, :cluster, :node_down]
        ])

      assert node() in Topology.nodes()
      assert :ok = Topology.remove_self()
      refute node() in Topology.nodes()

      assert_received {[:quanta, :cluster, :node_down], ^ref, %{count: _}, %{node: self_node}}
      assert self_node == node()
    after
      send(Process.whereis(Topology), {:nodeup, node(), []})
      _ = Topology.nodes()
    end

    test "idempotent when already removed" do
      Topology.remove_self()
      assert :ok = Topology.remove_self()
    after
      send(Process.whereis(Topology), {:nodeup, node(), []})
      _ = Topology.nodes()
    end
  end

  describe "CommandRouter.unsubscribe/0" do
    test "returns :ok even without NATS subscription" do
      assert :ok = CommandRouter.unsubscribe()
    end

    test "idempotent" do
      assert :ok = CommandRouter.unsubscribe()
      assert :ok = CommandRouter.unsubscribe()
    end
  end

  describe "Registry.mark_draining/1" do
    setup do
      :syn.add_node_to_scopes([:actors])
      :ok
    end

    test "sets draining flag in metadata" do
      actor_id = make_actor_id("drain-mark-1")
      :ok = Registry.register(actor_id)

      {_pid, meta_before} = :syn.lookup(:actors, actor_id)
      assert meta_before.draining == false

      assert {:ok, {_pid, meta_after}} = Registry.mark_draining(actor_id)
      assert meta_after.draining == true

      {_pid, meta_check} = :syn.lookup(:actors, actor_id)
      assert meta_check.draining == true
    end
  end

  describe "Registry.local_actor_ids/0" do
    setup do
      :syn.add_node_to_scopes([:actors])
      :ok
    end

    test "returns local registrations as {actor_id, pid, meta} tuples" do
      actor_id = make_actor_id("local-ids-1")
      :ok = Registry.register(actor_id)

      entries = Registry.local_actor_ids()
      assert is_list(entries)

      match = Enum.find(entries, fn {aid, _pid, _meta} -> aid == actor_id end)
      assert match != nil
      {^actor_id, pid, meta} = match
      assert pid == self()
      assert meta.draining == false
    end
  end
end
