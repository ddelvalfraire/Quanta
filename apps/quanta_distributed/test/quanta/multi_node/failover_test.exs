defmodule Quanta.MultiNode.FailoverTest do
  @moduledoc """
  MN2: Node failover — kill a node, verify actor re-activation on survivor.
  """

  use ExUnit.Case, async: false

  alias Quanta.ActorId
  alias Quanta.Envelope
  alias Quanta.Test.ClusterHelpers

  @moduletag :multi_node
  @moduletag timeout: 120_000

  setup_all do
    {:ok, cluster, nodes} = ClusterHelpers.start_cluster("mn2", 2)
    on_exit(fn -> ClusterHelpers.stop_cluster(cluster) end)
    {:ok, cluster: cluster, nodes: nodes, node_a: Enum.at(nodes, 0), node_b: Enum.at(nodes, 1)}
  end

  test "actor re-activates on survivor after node death", ctx do
    # Find an actor that hashes to node_b
    actor_id = find_actor_on(ctx.node_b)

    # Activate it
    envelope = Envelope.new(payload: "inc", sender: :system)
    assert {:ok, <<1::64>>} = ClusterHelpers.route_on(ctx.node_a, actor_id, envelope)

    # Verify it's on node_b
    assert {:ok, pid_before} = ClusterHelpers.cluster_lookup(ctx.node_a, actor_id)
    assert node(pid_before) == ctx.node_b

    # Kill node_b
    ClusterHelpers.stop_node(ctx.cluster, ctx.node_b)
    Process.sleep(1_000)

    # Route again — should re-activate on surviving node (node_a or manager)
    envelope2 = Envelope.new(payload: "inc", sender: :system)
    assert {:ok, _} = ClusterHelpers.route_on(ctx.node_a, actor_id, envelope2)

    # Actor should now be somewhere alive (not on dead node_b)
    assert {:ok, pid_after} = ClusterHelpers.cluster_lookup(ctx.node_a, actor_id)
    assert node(pid_after) != ctx.node_b
  end

  defp find_actor_on(target) do
    Enum.find_value(1..200, fn i ->
      id = %ActorId{namespace: "test", type: "counter", id: "failover-#{i}"}

      if :rpc.call(target, Quanta.Actor.Placement, :target_node, [id]) == target do
        id
      end
    end) || raise "No actor hashes to #{target}"
  end
end
