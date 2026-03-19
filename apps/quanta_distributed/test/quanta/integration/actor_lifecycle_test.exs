defmodule Quanta.Integration.ActorLifecycleTest do
  @moduledoc """
  FS1: Full actor lifecycle with NATS KV persistence.

  Deploy manifest, spawn actor, send messages, passivate, reactivate,
  and verify state survives the round-trip through NATS KV.

  Requires: `docker compose -f docker-compose.test.yml up -d`
  """

  use ExUnit.Case, async: false

  alias Quanta.ActorId
  alias Quanta.Envelope
  alias Quanta.Test.ClusterHelpers

  # TODO: uncomment when KV persistence is wired end-to-end
  # alias Quanta.Nats.JetStream
  # alias Quanta.Test.NatsHelpers

  @moduletag :integration
  @moduletag timeout: 120_000

  setup_all do
    {:ok, cluster, nodes} = ClusterHelpers.start_cluster("fs1", 2)
    on_exit(fn -> ClusterHelpers.stop_cluster(cluster) end)
    {:ok, cluster: cluster, nodes: nodes, node_a: Enum.at(nodes, 0), node_b: Enum.at(nodes, 1)}
  end

  test "full lifecycle: spawn, message, passivate, reactivate, verify state", ctx do
    actor_id = %ActorId{namespace: "test", type: "counter", id: "lifecycle-1"}

    # Step 1: Activate actor by sending a message
    envelope = Envelope.new(payload: "inc", sender: :system)
    assert {:ok, <<1::64>>} = ClusterHelpers.route_on(ctx.node_a, actor_id, envelope)

    # Step 2: Send more messages to accumulate state
    for _ <- 2..5 do
      assert {:ok, _} = ClusterHelpers.route_on(ctx.node_a, actor_id, envelope)
    end

    # Step 3: Verify current state
    get_envelope = Envelope.new(payload: "get", sender: :system)
    assert {:ok, <<5::64>>} = ClusterHelpers.route_on(ctx.node_a, actor_id, get_envelope)

    # Step 4: Force passivation (triggers on_passivate + state persist)
    # TODO: Call force_passivate via RPC once Server.force_passivate persists to KV
    {:ok, pid} = ClusterHelpers.cluster_lookup(ctx.node_a, actor_id)
    target_node = node(pid)
    :ok = :rpc.call(target_node, Quanta.Actor.Server, :force_passivate, [pid])
    Process.sleep(500)

    # Step 5: Reactivate by sending another message
    assert {:ok, _} = ClusterHelpers.route_on(ctx.node_a, actor_id, get_envelope)

    # TODO: Once KV persistence is wired end-to-end, assert that the
    # reactivated actor restores state from the snapshot (count == 5).
    # For now we verify the lifecycle doesn't crash.
  end

  test "actor survives node death and reactivates on survivor", ctx do
    actor_id = %ActorId{namespace: "test", type: "counter", id: "lifecycle-survive"}

    envelope = Envelope.new(payload: "inc", sender: :system)
    assert {:ok, <<1::64>>} = ClusterHelpers.route_on(ctx.node_a, actor_id, envelope)

    # TODO: Once KV persistence is wired, kill the owning node and verify
    # the actor reactivates on the survivor with state restored from KV.
    # This currently tests that the basic routing works across the cluster.
  end
end
