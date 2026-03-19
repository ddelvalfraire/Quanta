defmodule Quanta.Actor.PlacementTest do
  use ExUnit.Case, async: false

  alias Quanta.Actor.Placement
  alias Quanta.ActorId
  alias Quanta.Cluster.Topology

  @actor_id %ActorId{namespace: "test", type: "counter", id: "actor-1"}

  describe "target_node/1" do
    test "returns a node atom" do
      assert is_atom(Placement.target_node(@actor_id))
    end

    test "same actor_id always maps to the same node" do
      node1 = Placement.target_node(@actor_id)
      node2 = Placement.target_node(@actor_id)
      assert node1 == node2
    end

    test "returns current node in single-node setup" do
      assert Placement.target_node(@actor_id) == node()
    end
  end

  describe "target_nodes/2" do
    test "returns a list of the requested size in single-node setup" do
      assert [node()] == Placement.target_nodes(@actor_id, 1)
    end
  end

  describe "local?/1" do
    test "returns true in single-node setup" do
      assert Placement.local?(@actor_id)
    end
  end

  describe "with multiple nodes in ring" do
    @fake_nodes [:"placement-a@127.0.0.1", :"placement-b@127.0.0.1"]

    setup do
      for n <- @fake_nodes do
        send(Process.whereis(Topology), {:nodeup, n, []})
      end

      _ = Topology.nodes()

      on_exit(fn ->
        for n <- @fake_nodes do
          send(Process.whereis(Topology), {:nodedown, n, []})
        end

        _ = Topology.nodes()
      end)

      :ok
    end

    test "different actor_ids can map to different nodes" do
      ids =
        for i <- 1..50 do
          %ActorId{namespace: "test", type: "counter", id: "actor-#{i}"}
        end

      nodes = ids |> Enum.map(&Placement.target_node/1) |> Enum.uniq()

      assert length(nodes) >= 2
    end

    test "target_nodes/2 returns preference-ordered fallback list" do
      nodes = Placement.target_nodes(@actor_id, 2)
      assert length(nodes) == 2
      assert Enum.uniq(nodes) == nodes
    end

    test "node removal redistributes ~1/N of actors, not all" do
      all_nodes = [node() | @fake_nodes]
      actor_count = 300

      ids =
        for i <- 1..actor_count do
          %ActorId{namespace: "test", type: "counter", id: "rehash-#{i}"}
        end

      before = Enum.map(ids, &Placement.target_node/1)

      removed = :"placement-b@127.0.0.1"
      send(Process.whereis(Topology), {:nodedown, removed, []})
      _ = Topology.nodes()

      after_removal = Enum.map(ids, &Placement.target_node/1)

      changed =
        Enum.zip(before, after_removal)
        |> Enum.count(fn {b, a} -> b != a end)

      remaining_nodes = all_nodes -- [removed]
      assert Enum.all?(after_removal, &(&1 in remaining_nodes))
      assert changed >= actor_count * 0.10
      assert changed <= actor_count * 0.60

      send(Process.whereis(Topology), {:nodeup, removed, []})
      _ = Topology.nodes()
    end
  end
end
