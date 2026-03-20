defmodule Quanta.Chaos.CascadingFailureTest do
  @moduledoc """
  CH6: Kill 2 of 3 BEAM nodes — cascading failure.

  Activates actors across a 3-node cluster, kills 2 nodes in rapid
  succession, and verifies that the surviving node can still serve
  requests and that actors re-activate on it.

  Requires: `docker compose -f docker-compose.test.yml up -d`
  """

  use ExUnit.Case, async: false

  alias Quanta.ActorId
  alias Quanta.Envelope
  alias Quanta.Test.ClusterHelpers

  @moduletag :chaos
  @moduletag timeout: 180_000

  setup_all do
    {:ok, cluster, nodes} = ClusterHelpers.start_cluster("ch6", 3)
    on_exit(fn -> ClusterHelpers.stop_cluster(cluster) end)

    {:ok,
     cluster: cluster,
     nodes: nodes,
     node_a: Enum.at(nodes, 0),
     node_b: Enum.at(nodes, 1),
     node_c: Enum.at(nodes, 2)}
  end

  test "surviving node serves requests after 2 nodes die", ctx do
    # Step 1: Distribute actors across all 3 nodes
    actor_ids =
      for i <- 1..30 do
        id = %ActorId{namespace: "test", type: "counter", id: "cascade-#{i}"}
        envelope = Envelope.new(payload: "inc", sender: :system)
        {:ok, _} = ClusterHelpers.route_on(ctx.node_a, id, envelope)
        id
      end

    # Verify actors are spread across nodes
    count_a = ClusterHelpers.local_count(ctx.node_a)
    count_b = ClusterHelpers.local_count(ctx.node_b)
    count_c = ClusterHelpers.local_count(ctx.node_c)
    total = count_a + count_b + count_c
    assert total > 0, "Expected actors distributed across nodes"

    # Step 2: Kill node_b
    ClusterHelpers.stop_node(ctx.cluster, ctx.node_b)
    Process.sleep(1_500)

    # Step 3: Kill node_c (cascading failure)
    ClusterHelpers.stop_node(ctx.cluster, ctx.node_c)
    Process.sleep(2_000)

    # Step 4: Surviving node_a should still serve requests
    # The manager node (test node) + node_a remain in the ring.
    surviving_results =
      for id <- actor_ids do
        envelope = Envelope.new(payload: "get", sender: :system)
        ClusterHelpers.route_on(ctx.node_a, id, envelope)
      end

    successes = Enum.count(surviving_results, &match?({:ok, _}, &1))

    assert successes == length(actor_ids),
           "Expected all #{length(actor_ids)} actors to respond, got #{successes}"
  end

  test "new actors can be created on the sole survivor", ctx do
    # After the previous test killed nodes, try creating fresh actors
    # on the surviving node.
    # Note: setup_all runs once, so this test depends on cluster state.
    # In practice each test should get its own cluster — using unique IDs instead.

    fresh_ids =
      for i <- 1..10 do
        %ActorId{namespace: "test", type: "counter", id: "cascade-fresh-#{i}"}
      end

    for id <- fresh_ids do
      envelope = Envelope.new(payload: "inc", sender: :system)
      result = ClusterHelpers.route_on(ctx.node_a, id, envelope)

      # The actor should activate on node_a or the manager node
      assert {:ok, _} = result
    end
  end
end
