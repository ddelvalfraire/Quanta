defmodule Quanta.Chaos.SplitBrainTest do
  @moduledoc """
  CH4: BEAM cluster split-brain with concurrent writes.

  Uses Schism to partition the BEAM cluster into two halves,
  performs concurrent writes on both sides, heals the partition,
  and verifies that Syn conflict resolution produces a single winner.

  Requires: `docker compose -f docker-compose.test.yml up -d`
  """

  use ExUnit.Case, async: false

  alias Quanta.ActorId
  alias Quanta.Envelope
  alias Quanta.Test.ClusterHelpers

  @moduletag :chaos
  @moduletag timeout: 180_000

  setup_all do
    {:ok, cluster, nodes} = ClusterHelpers.start_cluster("ch4", 3)
    on_exit(fn -> ClusterHelpers.stop_cluster(cluster) end)

    {:ok,
     cluster: cluster,
     nodes: nodes,
     node_a: Enum.at(nodes, 0),
     node_b: Enum.at(nodes, 1),
     node_c: Enum.at(nodes, 2)}
  end

  test "split-brain heals with single winner after concurrent writes", ctx do
    actor_id = %ActorId{namespace: "test", type: "counter", id: "split-brain-1"}

    # Step 1: Activate actor
    envelope = Envelope.new(payload: "inc", sender: :system)
    assert {:ok, <<1::64>>} = ClusterHelpers.route_on(ctx.node_a, actor_id, envelope)

    # Step 2: Partition into two groups
    # Group 1: manager + node_a + node_b
    # Group 2: node_c (isolated)
    Schism.partition([node(), ctx.node_a, ctx.node_b])
    Schism.partition([ctx.node_c])
    Process.sleep(2_000)

    # Step 3: Concurrent writes on both sides of the partition
    task_a =
      Task.async(fn ->
        for _ <- 1..5 do
          ClusterHelpers.route_on(ctx.node_a, actor_id, envelope)
        end
      end)

    # Force-activate the same actor on the isolated node
    :rpc.call(ctx.node_c, Quanta.Actor.CommandRouter, :ensure_active_local, [actor_id])

    task_c =
      Task.async(fn ->
        for _ <- 1..5 do
          env = Envelope.new(payload: "inc", sender: :system)
          :rpc.call(ctx.node_c, Quanta.Actor.CommandRouter, :route_local, [actor_id, env, 10_000])
        end
      end)

    Task.await(task_a, 30_000)
    Task.await(task_c, 30_000)

    # Step 4: Heal the partition
    Schism.heal([node(), ctx.node_a, ctx.node_b, ctx.node_c])
    Process.sleep(3_000)

    # Step 5: After healing, Syn conflict resolution should leave one winner
    lookup_a = ClusterHelpers.cluster_lookup(ctx.node_a, actor_id)
    lookup_c = ClusterHelpers.cluster_lookup(ctx.node_c, actor_id)

    assert {:ok, winner_a} = lookup_a
    assert {:ok, winner_c} = lookup_c
    assert winner_a == winner_c, "Expected same winner on both sides after heal"
  end

  test "multiple actors survive split-brain and converge", ctx do
    actor_ids =
      for i <- 1..10 do
        %ActorId{namespace: "test", type: "counter", id: "split-multi-#{i}"}
      end

    # Activate all actors
    envelope = Envelope.new(payload: "inc", sender: :system)

    for id <- actor_ids do
      assert {:ok, _} = ClusterHelpers.route_on(ctx.node_a, id, envelope)
    end

    # Partition
    Schism.partition([node(), ctx.node_a])
    Schism.partition([ctx.node_b, ctx.node_c])
    Process.sleep(2_000)

    # Heal
    Schism.heal([node(), ctx.node_a, ctx.node_b, ctx.node_c])
    Process.sleep(3_000)

    # All actors should be resolvable from any node
    for id <- actor_ids do
      result = ClusterHelpers.route_on(ctx.node_a, id, Envelope.new(payload: "get", sender: :system))
      assert {:ok, _} = result
    end
  end
end
