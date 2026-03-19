defmodule Quanta.Actor.PlacementPropertyTest do
  @moduledoc """
  Property tests for hash ring placement.

  P2: Stability — same actor_id always maps to the same node.
       Minimal disruption — removing 1 of N nodes redistributes ≤ 2/N of actors.
  """

  use ExUnit.Case, async: false
  use PropCheck

  alias Quanta.Actor.Placement
  alias Quanta.ActorId
  alias Quanta.Cluster.Topology

  @moduletag :property

  # ── Generators ──────────────────────────────────────────────────────

  @segment_chars ~c"abcdefghijklmnopqrstuvwxyz0123456789_-"

  defp segment_gen do
    let chars <- non_empty(list(oneof(@segment_chars))) do
      List.to_string(chars)
    end
  end

  defp actor_id_gen do
    let {ns, type, id} <- {segment_gen(), segment_gen(), segment_gen()} do
      %ActorId{namespace: ns, type: type, id: id}
    end
  end

  # ── Properties ──────────────────────────────────────────────────────

  property "stability: target_node is deterministic for a given actor_id" do
    forall actor_id <- actor_id_gen() do
      node1 = Placement.target_node(actor_id)
      node2 = Placement.target_node(actor_id)
      node1 == node2
    end
  end

  property "target_node always returns a member of the current ring" do
    forall actor_id <- actor_id_gen() do
      target = Placement.target_node(actor_id)
      nodes = Topology.nodes()
      target in nodes
    end
  end

  describe "minimal disruption bound" do
    @fake_nodes [
      :"prop-a@127.0.0.1",
      :"prop-b@127.0.0.1",
      :"prop-c@127.0.0.1",
      :"prop-d@127.0.0.1"
    ]

    setup do
      for n <- @fake_nodes do
        send(Process.whereis(Topology), {:nodeup, n, []})
      end

      # Wait for ring to stabilize
      _ = Topology.nodes()

      on_exit(fn ->
        for n <- @fake_nodes do
          send(Process.whereis(Topology), {:nodedown, n, []})
        end

        _ = Topology.nodes()
      end)

      :ok
    end

    test "removing 1 of N nodes redistributes at most 2/N of actors" do
      all_nodes = Topology.nodes()
      n = length(all_nodes)
      actor_count = 1000

      ids =
        for i <- 1..actor_count do
          %ActorId{namespace: "test", type: "counter", id: "prop-rehash-#{i}"}
        end

      before = Enum.map(ids, &Placement.target_node/1)

      # Remove one fake node
      removed = hd(@fake_nodes)
      send(Process.whereis(Topology), {:nodedown, removed, []})
      _ = Topology.nodes()

      after_removal = Enum.map(ids, &Placement.target_node/1)

      changed = Enum.zip(before, after_removal) |> Enum.count(fn {b, a} -> b != a end)

      # Upper bound: 2/N of actors should move (generous bound for consistent hashing)
      max_disruption = actor_count * 2.0 / n
      assert changed <= max_disruption,
             "#{changed} actors moved, expected at most #{round(max_disruption)} (2/#{n})"

      # Lower bound: at least some actors should move (the ones that were on removed node)
      assert changed > 0, "expected some actors to move after removing a node"

      # Restore
      send(Process.whereis(Topology), {:nodeup, removed, []})
      _ = Topology.nodes()
    end
  end
end
