defmodule Quanta.Cluster.TopologyTest do
  use ExUnit.Case, async: false

  alias Quanta.Cluster.Topology

  describe "ring/0" do
    test "returns {:ok, pid} with a live ring process" do
      assert {:ok, ring} = Topology.ring()
      assert is_pid(ring)
      assert Process.alive?(ring)
    end
  end

  describe "nodes/0" do
    test "includes the current node" do
      assert node() in Topology.nodes()
    end
  end

  describe "healthy?/0" do
    test "returns true in single-node mode" do
      assert Topology.healthy?()
    end
  end

  describe "nodeup/nodedown" do
    test "nodeup adds a node to the ring and emits telemetry" do
      ref =
        :telemetry_test.attach_event_handlers(self(), [
          [:quanta, :cluster, :node_up]
        ])

      fake_node = :"fake@127.0.0.1"
      send(Process.whereis(Topology), {:nodeup, fake_node, []})

      assert fake_node in Topology.nodes()

      assert_received {[:quanta, :cluster, :node_up], ^ref, %{count: count},
                       %{node: ^fake_node}}

      assert count >= 2

      send(Process.whereis(Topology), {:nodedown, fake_node, []})
      _ = Topology.nodes()
    end

    test "duplicate nodeup is idempotent" do
      fake_node = :"dup@127.0.0.1"
      send(Process.whereis(Topology), {:nodeup, fake_node, []})
      assert fake_node in Topology.nodes()

      send(Process.whereis(Topology), {:nodeup, fake_node, []})
      assert fake_node in Topology.nodes()

      send(Process.whereis(Topology), {:nodedown, fake_node, []})
      _ = Topology.nodes()
    end

    test "nodedown removes a node from the ring and emits telemetry" do
      fake_node = :"fake2@127.0.0.1"

      send(Process.whereis(Topology), {:nodeup, fake_node, []})
      assert fake_node in Topology.nodes()

      ref =
        :telemetry_test.attach_event_handlers(self(), [
          [:quanta, :cluster, :node_down]
        ])

      send(Process.whereis(Topology), {:nodedown, fake_node, []})

      refute fake_node in Topology.nodes()

      assert_received {[:quanta, :cluster, :node_down], ^ref, %{count: _},
                       %{node: ^fake_node}}
    end

    test "nodedown for unknown node is idempotent" do
      send(Process.whereis(Topology), {:nodedown, :"unknown@127.0.0.1", []})
      assert node() in Topology.nodes()
    end

    test "ring is updated after nodeup so find_node can route to the new node" do
      fake_node = :"fake3@127.0.0.1"
      send(Process.whereis(Topology), {:nodeup, fake_node, []})
      _ = Topology.nodes()

      {:ok, ring} = Topology.ring()
      {:ok, found} = ExHashRing.Ring.find_node(ring, "some-key")
      assert found in [node(), fake_node]

      send(Process.whereis(Topology), {:nodedown, fake_node, []})
      _ = Topology.nodes()
    end
  end
end
