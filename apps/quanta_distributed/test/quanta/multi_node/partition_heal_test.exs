defmodule Quanta.MultiNode.PartitionHealTest do
  @moduledoc """
  MN3: Network partition and heal — Schism partition, dual activation,
  heal, verify one winner via Syn conflict resolution.
  """

  use ExUnit.Case, async: false

  alias Quanta.ActorId
  alias Quanta.Envelope
  alias Quanta.Test.ClusterHelpers

  @moduletag :multi_node
  @moduletag timeout: 120_000

  setup_all do
    {:ok, cluster, nodes} = ClusterHelpers.start_cluster("mn3", 2)
    on_exit(fn -> ClusterHelpers.stop_cluster(cluster) end)
    {:ok, cluster: cluster, nodes: nodes, node_a: Enum.at(nodes, 0), node_b: Enum.at(nodes, 1)}
  end

  test "partition heals and conflict resolution picks one winner", ctx do
    actor_id = %ActorId{namespace: "test", type: "counter", id: "partition-heal"}

    # Activate actor on node_a
    envelope = Envelope.new(payload: "inc", sender: :system)
    assert {:ok, _} = ClusterHelpers.route_on(ctx.node_a, actor_id, envelope)

    # Partition: isolate node_b from node_a
    Schism.partition([ctx.node_a, node()])
    Schism.partition([ctx.node_b])
    Process.sleep(1_000)

    # Activate same actor on node_b (dual activation during partition)
    envelope2 = Envelope.new(payload: "inc", sender: :system)
    :rpc.call(ctx.node_b, Quanta.Actor.CommandRouter, :ensure_active_local, [actor_id])

    # Heal the partition
    Schism.heal([ctx.node_a, ctx.node_b, node()])
    Process.sleep(2_000)

    # After heal, Syn conflict resolution should leave exactly one winner
    lookup_a = ClusterHelpers.cluster_lookup(ctx.node_a, actor_id)
    lookup_b = ClusterHelpers.cluster_lookup(ctx.node_b, actor_id)

    # Both nodes should agree on the same pid
    assert {:ok, winner_pid} = lookup_a
    assert {:ok, ^winner_pid} = lookup_b
  end
end
