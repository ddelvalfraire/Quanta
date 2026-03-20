defmodule Quanta.MultiNode.ChannelReconnectTest do
  @moduledoc """
  MN5: Drain notification + client reconnect to survivor.

  Verifies that after a drain notification, actors can be re-activated
  on surviving nodes.
  """

  use ExUnit.Case, async: false

  alias Quanta.ActorId
  alias Quanta.Envelope
  alias Quanta.Test.ClusterHelpers

  @moduletag :multi_node
  @moduletag timeout: 120_000

  setup_all do
    {:ok, cluster, nodes} = ClusterHelpers.start_cluster("mn5", 2)
    on_exit(fn -> ClusterHelpers.stop_cluster(cluster) end)
    {:ok, cluster: cluster, nodes: nodes, node_a: Enum.at(nodes, 0), node_b: Enum.at(nodes, 1)}
  end

  test "after drain, actors can be reached on survivor node", ctx do
    actor_id = find_actor_on(ctx.node_a, "reconnect")

    # Activate on node_a
    envelope = Envelope.new(payload: "inc", sender: :system)
    assert {:ok, <<1::64>>} = ClusterHelpers.route_on(ctx.node_a, actor_id, envelope)

    # Trigger drain on node_a
    drain_opts = [
      complete_in_flight_delay_ms: 100,
      ordered_passivation_delay_ms: 100,
      total_drain_budget_ms: 10_000
    ]

    {:ok, _} = :rpc.call(ctx.node_a, Quanta.Drain, :start, [drain_opts])
    Process.sleep(2_000)

    # "Reconnect" — route the same actor from node_b
    # It should activate on a surviving node (node_b or manager)
    envelope2 = Envelope.new(payload: "inc", sender: :system)
    result = ClusterHelpers.route_on(ctx.node_b, actor_id, envelope2)

    # The actor re-activated fresh (state was lost since no NATS persistence in this test)
    assert {:ok, _} = result
  end

  defp find_actor_on(target, suffix) do
    Enum.find_value(1..200, fn i ->
      id = %ActorId{namespace: "test", type: "counter", id: "#{suffix}-#{i}"}

      if :rpc.call(target, Quanta.Actor.Placement, :target_node, [id]) == target do
        id
      end
    end) || raise "No actor hashes to #{target}"
  end
end
