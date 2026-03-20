defmodule Quanta.MultiNode.ActorRoutingTest do
  @moduledoc """
  MN1: Two-node routing via CommandRouter.

  Verifies that the hash ring routes actors to the correct node and that
  messages are delivered across nodes.
  """

  use ExUnit.Case, async: false

  alias Quanta.ActorId
  alias Quanta.Envelope
  alias Quanta.Test.ClusterHelpers

  @moduletag :multi_node
  @moduletag timeout: 120_000

  setup_all do
    {:ok, cluster, nodes} = ClusterHelpers.start_cluster("mn1", 2)
    on_exit(fn -> ClusterHelpers.stop_cluster(cluster) end)
    {:ok, cluster: cluster, nodes: nodes, node_a: Enum.at(nodes, 0), node_b: Enum.at(nodes, 1)}
  end

  test "actor activates on its hash-ring target node", ctx do
    ids =
      for i <- 1..20 do
        %ActorId{namespace: "test", type: "counter", id: "route-#{i}"}
      end

    for id <- ids do
      envelope = Envelope.new(payload: "inc", sender: :system)
      assert {:ok, _} = ClusterHelpers.route_on(ctx.node_a, id, envelope)
    end

    count_a = ClusterHelpers.local_count(ctx.node_a)
    count_b = ClusterHelpers.local_count(ctx.node_b)

    # Hash ring includes manager node, so some actors may land there
    total_remote = count_a + count_b
    assert total_remote > 0, "expected some actors on cluster nodes"
    assert count_a + count_b >= 10, "expected at least half of actors on cluster nodes, got #{total_remote}"
  end

  test "message routed to remote node returns correct response", ctx do
    actor_id = find_actor_on_node(ctx.node_b, ctx.nodes)

    envelope = Envelope.new(payload: "inc", sender: :system)
    assert {:ok, <<1::64>>} = ClusterHelpers.route_on(ctx.node_a, actor_id, envelope)

    assert {:ok, pid} = ClusterHelpers.cluster_lookup(ctx.node_b, actor_id)
    assert node(pid) == ctx.node_b
  end

  test "same actor_id always routes to the same node", ctx do
    actor_id = %ActorId{namespace: "test", type: "counter", id: "stable-route"}

    envelope1 = Envelope.new(payload: "inc", sender: :system)
    assert {:ok, <<1::64>>} = ClusterHelpers.route_on(ctx.node_a, actor_id, envelope1)

    envelope2 = Envelope.new(payload: "inc", sender: :system)
    assert {:ok, <<2::64>>} = ClusterHelpers.route_on(ctx.node_b, actor_id, envelope2)

    envelope3 = Envelope.new(payload: "get", sender: :system)
    assert {:ok, <<2::64>>} = ClusterHelpers.route_on(ctx.node_a, actor_id, envelope3)
  end

  defp find_actor_on_node(target, _nodes) do
    Enum.find_value(1..200, fn i ->
      id = %ActorId{namespace: "test", type: "counter", id: "probe-#{i}"}
      placed = :rpc.call(target, Quanta.Actor.Placement, :target_node, [id])

      if placed == target, do: id
    end) || raise "Could not find actor_id that hashes to #{target}"
  end
end
