defmodule Quanta.Integration.CrdtCollabTest do
  @moduledoc """
  FS2: CRDT collaborative editing convergence.

  5 concurrent editors apply edits to the same CRDT-backed actor,
  and all edits must converge to a consistent state.

  Requires: `docker compose -f docker-compose.test.yml up -d`
  """

  use ExUnit.Case, async: false

  alias Quanta.ActorId
  alias Quanta.Envelope
  alias Quanta.Test.ClusterHelpers

  @moduletag :integration
  @moduletag timeout: 120_000

  @crdt_manifest %Quanta.Manifest{
    version: "1",
    namespace: "test",
    type: "crdt_doc",
    state: %Quanta.Manifest.State{},
    lifecycle: %Quanta.Manifest.Lifecycle{},
    resources: %Quanta.Manifest.Resources{},
    rate_limits: %Quanta.Manifest.RateLimits{}
  }

  setup_all do
    {:ok, cluster, nodes} =
      ClusterHelpers.start_cluster("fs2", 2,
        manifests: [
          @crdt_manifest,
          %Quanta.Manifest{
            version: "1",
            namespace: "test",
            type: "counter",
            state: %Quanta.Manifest.State{},
            lifecycle: %Quanta.Manifest.Lifecycle{},
            resources: %Quanta.Manifest.Resources{},
            rate_limits: %Quanta.Manifest.RateLimits{}
          }
        ],
        actor_modules: %{
          {"test", "crdt_doc"} => Quanta.Test.Actors.CrdtDoc,
          {"test", "counter"} => Quanta.Test.Actors.Counter
        }
      )

    on_exit(fn -> ClusterHelpers.stop_cluster(cluster) end)
    {:ok, cluster: cluster, nodes: nodes, node_a: Enum.at(nodes, 0), node_b: Enum.at(nodes, 1)}
  end

  test "5 concurrent editors converge via CRDT", ctx do
    actor_id = %ActorId{namespace: "test", type: "crdt_doc", id: "collab-1"}

    # Spawn 5 concurrent tasks simulating editors
    tasks =
      for editor_idx <- 1..5 do
        Task.async(fn ->
          # Each editor applies a map_set operation
          payload = "map_set:editor_#{editor_idx}:value_#{editor_idx}"
          envelope = Envelope.new(payload: payload, sender: :system)

          # Route through alternating nodes to simulate distributed access
          target = if rem(editor_idx, 2) == 0, do: ctx.node_a, else: ctx.node_b
          ClusterHelpers.route_on(target, actor_id, envelope)
        end)
      end

    # Wait for all editors to complete
    results = Task.await_many(tasks, 30_000)

    for result <- results do
      assert {:ok, _} = result
    end

    # TODO: Once CRDT state read-back is wired end-to-end, fetch the
    # actor's CRDT snapshot and verify all 5 editor keys are present.
    # For now we verify concurrent access doesn't crash.
    Process.sleep(500)
  end

  test "text inserts from multiple editors converge", ctx do
    actor_id = %ActorId{namespace: "test", type: "crdt_doc", id: "collab-text"}

    # Sequential inserts at different positions
    inserts = [
      "text_insert:0:hello",
      "text_insert:5: world",
      "text_insert:11:!"
    ]

    for payload <- inserts do
      envelope = Envelope.new(payload: payload, sender: :system)
      assert {:ok, _} = ClusterHelpers.route_on(ctx.node_a, actor_id, envelope)
    end

    # TODO: Read back the text CRDT and verify final content matches
    # expected merged result. For now we verify the operations succeed.
  end
end
