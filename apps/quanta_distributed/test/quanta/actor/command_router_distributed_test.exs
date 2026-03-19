defmodule Quanta.Actor.CommandRouterDistributedTest do
  use ExUnit.Case, async: false

  @moduletag :distributed

  alias Quanta.Actor.{CommandRouter, DynSup, ManifestRegistry, Registry}
  alias Quanta.{ActorId, Envelope, Manifest, RateLimit}
  alias Quanta.Cluster.Topology
  alias Quanta.Test.Actors.Counter

  @namespace "test"
  @type_name "counter"

  defp build_manifest(overrides \\ []) do
    attrs =
      Keyword.merge(
        [version: "1", type: @type_name, namespace: @namespace],
        overrides
      )

    struct!(Manifest, attrs)
  end

  defp make_envelope(payload) do
    Envelope.new(payload: payload)
  end

  defp clear_dynsup do
    PartitionSupervisor.which_children(DynSup)
    |> Enum.each(fn {_id, sup_pid, _type, _modules} ->
      DynamicSupervisor.which_children(sup_pid)
      |> Enum.each(fn {_, child_pid, _, _} ->
        DynamicSupervisor.terminate_child(sup_pid, child_pid)
      end)
    end)
  end

  # Find an actor_id whose hash ring placement targets the given node.
  defp find_actor_id_targeting(target_node) do
    Enum.find_value(1..10_000, fn i ->
      actor_id = %ActorId{namespace: @namespace, type: @type_name, id: "dist-#{i}"}

      if Quanta.Actor.Placement.target_node(actor_id) == target_node do
        actor_id
      end
    end) || raise "Could not find actor_id targeting #{target_node} after 10000 attempts"
  end

  setup do
    # Ensure distribution is started
    unless Node.alive?() do
      {:ok, _} =
        :net_kernel.start([
          :"quanta_cmd_test_#{System.unique_integer([:positive])}",
          :shortnames
        ])
    end

    clear_dynsup()

    if :ets.whereis(Quanta.RateLimit) != :undefined do
      :ets.delete(Quanta.RateLimit)
    end

    RateLimit.init()

    :sys.replace_state(ManifestRegistry, fn state ->
      :ets.delete_all_objects(state)
      state
    end)

    :ok = ManifestRegistry.put(build_manifest())

    prev_modules = Application.get_env(:quanta_distributed, :actor_modules, %{})

    Application.put_env(:quanta_distributed, :actor_modules, %{
      {@namespace, @type_name} => Counter
    })

    on_exit(fn ->
      Application.put_env(:quanta_distributed, :actor_modules, prev_modules)
    end)

    :ok
  end

  describe "distributed routing via hash ring" do
    setup do
      # Start peer node
      peer_name = :"peer_cmd_#{System.unique_integer([:positive])}"
      code_paths = :code.get_path() |> Enum.flat_map(fn path -> [~c"-pa", path] end)

      {:ok, peer, peer_node} =
        :peer.start(%{name: peer_name, connection: :standard_io, args: code_paths})

      # Boot applications on peer (these survive :peer.call because
      # they're owned by the application controller, not the call worker)
      for app <- [:elixir, :logger, :telemetry, :syn, :ex_hash_ring] do
        {:ok, _} = :peer.call(peer, :application, :ensure_all_started, [app])
      end

      :ok = :peer.call(peer, :syn, :add_node_to_scopes, [[:actors]])
      :ok = :peer.call(peer, :syn, :set_event_handler, [Quanta.Actor.SynEventHandler])

      # Boot actor infrastructure on peer via long-lived holder process
      {:ok, holder} =
        :peer.call(peer, Quanta.Test.PeerInfra, :boot, [
          [
            manifests: [build_manifest()],
            actor_modules: %{{@namespace, @type_name} => Counter}
          ]
        ])

      # Add the peer to our topology ring so hash ring knows about it
      send(Process.whereis(Topology), {:nodeup, peer_node, []})
      _ = Topology.nodes()

      # Wait for Syn to sync scopes
      Process.sleep(300)

      on_exit(fn ->
        send(Process.whereis(Topology), {:nodedown, peer_node, []})
        _ = Topology.nodes()

        try do
          send(holder, :stop)
          :peer.stop(peer)
        catch
          _, _ -> :ok
        end
      end)

      {:ok, peer: peer, peer_node: peer_node}
    end

    test "routes to existing remote actor via Syn cross-node lookup", ctx do
      actor_id = find_actor_id_targeting(ctx.peer_node)

      # Start an actor on the peer
      {:ok, remote_pid} =
        :peer.call(ctx.peer, DynSup, :start_actor, [
          actor_id,
          [actor_id: actor_id, module: Counter]
        ])

      # Wait for Syn to propagate the registration
      Process.sleep(200)

      # Route should find it via Syn and deliver directly
      envelope = make_envelope("inc")
      assert {:ok, <<1::64>>} = CommandRouter.route(actor_id, envelope)

      # Verify it's still running on the remote node
      assert node(remote_pid) == ctx.peer_node
    end

    test "activates passivated actor on hash ring target (remote) node", ctx do
      actor_id = find_actor_id_targeting(ctx.peer_node)

      # Actor is not active anywhere
      assert :not_found = Registry.lookup(actor_id)

      # Route should forward activation to the peer via RPC
      envelope = make_envelope("inc")
      assert {:ok, <<1::64>>} = CommandRouter.route(actor_id, envelope)

      # Actor should now be registered (Syn cross-node)
      Process.sleep(200)
      assert {:ok, pid} = Registry.lookup(actor_id)
      assert node(pid) == ctx.peer_node
    end

    test "remote ensure_active/1 returns {:ok, pid}", ctx do
      actor_id = find_actor_id_targeting(ctx.peer_node)

      assert {:ok, pid} = CommandRouter.ensure_active(actor_id)
      assert node(pid) == ctx.peer_node
    end

    test "node-down fallback activates locally" do
      # Add a fake node to the ring
      fake_node = :"dead_node_#{System.unique_integer([:positive])}@127.0.0.1"
      send(Process.whereis(Topology), {:nodeup, fake_node, []})
      _ = Topology.nodes()

      on_exit(fn ->
        send(Process.whereis(Topology), {:nodedown, fake_node, []})
        _ = Topology.nodes()
      end)

      actor_id = find_actor_id_targeting(fake_node)

      # Route should try RPC to dead node, fail, and fall back to local
      envelope = make_envelope("inc")
      assert {:ok, <<1::64>>} = CommandRouter.route(actor_id, envelope)

      # Actor should be running locally
      assert {:ok, pid} = Registry.lookup(actor_id)
      assert node(pid) == node()
    end

    test "concurrent activations on remote node both succeed", ctx do
      actor_id = find_actor_id_targeting(ctx.peer_node)

      tasks =
        for _ <- 1..10 do
          Task.async(fn ->
            CommandRouter.route(actor_id, make_envelope("inc"))
          end)
        end

      results = Task.await_many(tasks, 15_000)

      assert Enum.all?(results, fn
               {:ok, _} -> true
               _ -> false
             end)

      # Final count should be 10
      assert {:ok, <<10::64>>} = CommandRouter.route(actor_id, make_envelope("get"))
    end
  end

  describe "single-node mode" do
    test "degenerates to Phase 1 behavior (all local)" do
      # In single-node mode, all actors hash to local node
      actor_id = %ActorId{namespace: @namespace, type: @type_name, id: "single-node-1"}

      envelope = make_envelope("inc")
      assert {:ok, <<1::64>>} = CommandRouter.route(actor_id, envelope)

      assert {:ok, pid} = Registry.lookup(actor_id)
      assert node(pid) == node()
    end
  end
end
