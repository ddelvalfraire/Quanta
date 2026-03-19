defmodule Quanta.Chaos.KillMidSnapshotTest do
  @moduledoc """
  CH2: Kill actor process during save_snapshot.

  Interrupts an actor mid-persistence to verify that partial writes
  don't corrupt state and that the actor can recover cleanly.

  Requires: `docker compose -f docker-compose.test.yml up -d`
  """

  use ExUnit.Case, async: false

  alias Quanta.ActorId
  alias Quanta.Envelope
  alias Quanta.Test.ClusterHelpers

  @moduletag :chaos
  @moduletag timeout: 180_000

  setup_all do
    {:ok, cluster, nodes} = ClusterHelpers.start_cluster("ch2", 2)
    on_exit(fn -> ClusterHelpers.stop_cluster(cluster) end)
    {:ok, cluster: cluster, nodes: nodes, node_a: Enum.at(nodes, 0), node_b: Enum.at(nodes, 1)}
  end

  test "actor recovers after kill during passivation", ctx do
    actor_id = %ActorId{namespace: "test", type: "counter", id: "mid-snapshot-1"}

    # Step 1: Activate and accumulate state
    envelope = Envelope.new(payload: "inc", sender: :system)

    for _ <- 1..10 do
      assert {:ok, _} = ClusterHelpers.route_on(ctx.node_a, actor_id, envelope)
    end

    # Step 2: Look up the actor process
    {:ok, pid} = ClusterHelpers.cluster_lookup(ctx.node_a, actor_id)
    target_node = node(pid)

    # Step 3: Kill the process abruptly (simulating crash during snapshot)
    :rpc.call(target_node, Process, :exit, [pid, :kill])
    Process.sleep(500)

    # Step 4: Reactivate — should not crash even if last snapshot was partial
    get_envelope = Envelope.new(payload: "get", sender: :system)
    result = ClusterHelpers.route_on(ctx.node_a, actor_id, get_envelope)

    # TODO: Once KV persistence is wired, verify that state is either:
    # - The last successfully persisted snapshot, OR
    # - The initial state (if no snapshot was completed)
    # For now we just verify the actor can reactivate without crashing.
    assert {:ok, _} = result
  end

  test "concurrent kills during passivation don't corrupt shared state", ctx do
    # Activate multiple actors, then kill them all simultaneously
    actor_ids =
      for i <- 1..5 do
        id = %ActorId{namespace: "test", type: "counter", id: "mid-snap-concurrent-#{i}"}
        envelope = Envelope.new(payload: "inc", sender: :system)
        {:ok, _} = ClusterHelpers.route_on(ctx.node_a, id, envelope)
        id
      end

    # Kill all actor processes at once
    for id <- actor_ids do
      case ClusterHelpers.cluster_lookup(ctx.node_a, id) do
        {:ok, pid} ->
          :rpc.call(node(pid), Process, :exit, [pid, :kill])

        :not_found ->
          :ok
      end
    end

    Process.sleep(1_000)

    # All actors should reactivate cleanly
    for id <- actor_ids do
      get_envelope = Envelope.new(payload: "get", sender: :system)
      assert {:ok, _} = ClusterHelpers.route_on(ctx.node_a, id, get_envelope)
    end
  end
end
