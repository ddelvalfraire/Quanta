defmodule Quanta.MultiNode.DrainTest do
  @moduledoc """
  MN4: Drain on node A — verify actors passivate.
  """

  use ExUnit.Case, async: false

  alias Quanta.ActorId
  alias Quanta.Envelope
  alias Quanta.Test.ClusterHelpers

  @moduletag :multi_node
  @moduletag timeout: 120_000

  setup_all do
    {:ok, cluster, nodes} = ClusterHelpers.start_cluster("mn4", 2)
    on_exit(fn -> ClusterHelpers.stop_cluster(cluster) end)
    {:ok, cluster: cluster, nodes: nodes, node_a: Enum.at(nodes, 0), node_b: Enum.at(nodes, 1)}
  end

  test "drain passivates actors on the draining node", ctx do
    # Activate some actors on node_a
    actor_ids = for i <- 1..5, into: [] do
      id = find_actor_on(ctx.node_a, "drain-#{i}")
      envelope = Envelope.new(payload: "inc", sender: :system)
      {:ok, _} = ClusterHelpers.route_on(ctx.node_a, id, envelope)
      id
    end

    count_before = ClusterHelpers.local_count(ctx.node_a)
    assert count_before >= 5

    # Trigger drain with short timeouts
    drain_opts = [
      complete_in_flight_delay_ms: 100,
      ordered_passivation_delay_ms: 100,
      total_drain_budget_ms: 10_000
    ]

    {:ok, _pid} = :rpc.call(ctx.node_a, Quanta.Drain, :start, [drain_opts])

    # Wait for drain to complete
    Process.sleep(3_000)

    # Verify draining flag is set
    assert :rpc.call(ctx.node_a, Quanta.Drain, :draining?, []) == true

    # Actors should have been passivated (count should be reduced)
    count_after = ClusterHelpers.local_count(ctx.node_a)
    assert count_after < count_before, "expected fewer actors after drain"
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
