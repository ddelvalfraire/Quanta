defmodule Quanta.Actor.CommandRouterTest do
  use ExUnit.Case, async: false

  alias Quanta.Actor.{CommandRouter, DynSup, ManifestRegistry, Registry, Server}
  alias Quanta.{ActorId, Envelope, Manifest, RateLimit}
  alias Quanta.Test.Actors.{Counter, Echo}

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

  defp make_actor_id(id, type \\ @type_name) do
    %ActorId{namespace: @namespace, type: type, id: id}
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

  setup do
    clear_dynsup()

    # Ensure clean RateLimit ETS table
    if :ets.whereis(Quanta.RateLimit) != :undefined do
      :ets.delete(Quanta.RateLimit)
    end

    RateLimit.init()

    :sys.replace_state(ManifestRegistry, fn state ->
      :ets.delete_all_objects(state)
      state
    end)

    :ok = ManifestRegistry.put(build_manifest())
    :ok = ManifestRegistry.put(build_manifest(type: "echo"))

    # Register actor modules for the router to find
    prev = Application.get_env(:quanta_distributed, :actor_modules, %{})

    Application.put_env(:quanta_distributed, :actor_modules, %{
      {@namespace, @type_name} => Counter,
      {@namespace, "echo"} => Echo
    })

    on_exit(fn ->
      Application.put_env(:quanta_distributed, :actor_modules, prev)
    end)

    :ok
  end

  describe "route/3 — routing to existing actor" do
    test "routes message to an active actor" do
      actor_id = make_actor_id("rt-existing")
      {:ok, _pid} = start_actor(actor_id)

      envelope = make_envelope("inc")
      assert {:ok, <<1::64>>} = CommandRouter.route(actor_id, envelope)
    end

    test "returns :no_reply when actor produces no reply" do
      actor_id = make_actor_id("rt-noreply")
      {:ok, _pid} = start_actor(actor_id)

      envelope = make_envelope("no_reply")
      assert {:ok, :no_reply} = CommandRouter.route(actor_id, envelope)
    end
  end

  describe "route/3 — activation on demand" do
    test "activates a passivated actor and delivers message" do
      actor_id = make_actor_id("rt-activate")
      assert :not_found = Registry.lookup(actor_id)

      envelope = make_envelope("inc")
      assert {:ok, <<1::64>>} = CommandRouter.route(actor_id, envelope)

      # Actor is now active in the registry
      assert {:ok, _pid} = Registry.lookup(actor_id)
    end

    test "multiple increments work on activated actor" do
      actor_id = make_actor_id("rt-multi")

      assert {:ok, <<1::64>>} = CommandRouter.route(actor_id, make_envelope("inc"))
      assert {:ok, <<2::64>>} = CommandRouter.route(actor_id, make_envelope("inc"))
      assert {:ok, <<2::64>>} = CommandRouter.route(actor_id, make_envelope("get"))
    end
  end

  describe "route/3 — race condition handling" do
    test "concurrent activations of same actor both succeed" do
      actor_id = make_actor_id("rt-race")

      tasks =
        for _ <- 1..10 do
          Task.async(fn ->
            CommandRouter.route(actor_id, make_envelope("inc"))
          end)
        end

      results = Task.await_many(tasks, 10_000)

      # All should succeed (no crashes)
      assert Enum.all?(results, fn
               {:ok, _} -> true
               _ -> false
             end)

      # Final count should be 10
      assert {:ok, <<10::64>>} = CommandRouter.route(actor_id, make_envelope("get"))
    end
  end

  describe "route/3 — error cases" do
    test "returns :actor_type_not_found for unknown type" do
      actor_id = %ActorId{namespace: @namespace, type: "nonexistent", id: "x"}
      envelope = make_envelope("hello")

      assert {:error, :actor_type_not_found} = CommandRouter.route(actor_id, envelope)
    end

    test "returns :rate_limited when rate exceeded" do
      rate_limits = %Quanta.Manifest.RateLimits{
        messages_per_second: 2,
        messages_per_second_type: 100_000
      }

      :ok = ManifestRegistry.put(build_manifest(type: "limited", rate_limits: rate_limits))

      Application.put_env(:quanta_distributed, :actor_modules, %{
        {@namespace, @type_name} => Counter,
        {@namespace, "echo"} => Echo,
        {@namespace, "limited"} => Counter
      })

      actor_id = %ActorId{namespace: @namespace, type: "limited", id: "rl-1"}

      # Consume the 2 allowed tokens
      assert {:ok, _} = CommandRouter.route(actor_id, make_envelope("inc"))
      assert {:ok, _} = CommandRouter.route(actor_id, make_envelope("inc"))

      # Third should be rate limited
      assert {:error, :rate_limited} = CommandRouter.route(actor_id, make_envelope("inc"))

      Quanta.RateLimit.reset(actor_id)
    end

    test "returns :node_at_capacity when max_actors exceeded" do
      prev = Application.get_env(:quanta_distributed, :max_actors_per_node)
      Application.put_env(:quanta_distributed, :max_actors_per_node, 0)

      actor_id = make_actor_id("rt-capacity")
      envelope = make_envelope("inc")

      assert {:error, :node_at_capacity} = CommandRouter.route(actor_id, envelope)

      Application.put_env(:quanta_distributed, :max_actors_per_node, prev || 1_000_000)
    end

    test "returns :module_not_configured when actor module mapping is missing" do
      :ok = ManifestRegistry.put(build_manifest(type: "unmapped"))
      actor_id = %ActorId{namespace: @namespace, type: "unmapped", id: "x"}

      assert {:error, :module_not_configured} = CommandRouter.route(actor_id, make_envelope("hi"))
    end

    test "returns :actor_timeout when actor is slow" do
      actor_id = make_actor_id("rt-timeout")
      {:ok, pid} = start_actor(actor_id)

      # Suspend the actor process so it can't respond
      :sys.suspend(pid)

      envelope = make_envelope("inc")
      assert {:error, :actor_timeout} = CommandRouter.route(actor_id, envelope, 100)

      :sys.resume(pid)
    end
  end

  describe "route/3 — HTTP-only mode (no NATS)" do
    test "route/3 works without any NATS subscription" do
      actor_id = make_actor_id("http-only")
      envelope = make_envelope("inc")

      assert {:ok, <<1::64>>} = CommandRouter.route(actor_id, envelope)
    end
  end

  describe "parse_command_subject/1" do
    test "parses valid command subject" do
      assert {:ok, %ActorId{namespace: "myapp", type: "counter", id: "abc123"}} =
               CommandRouter.parse_command_subject("quanta.myapp.cmd.counter.abc123")
    end

    test "rejects subject with extra dot-separated segments in id (ids are NATS-safe)" do
      assert {:error, _} =
               CommandRouter.parse_command_subject("quanta.ns.cmd.player.region.server.user123")
    end

    test "parses subject with valid id containing hyphens and underscores" do
      assert {:ok, %ActorId{namespace: "ns", type: "player", id: "user-123_abc"}} =
               CommandRouter.parse_command_subject("quanta.ns.cmd.player.user-123_abc")
    end

    test "rejects subject with wrong prefix" do
      assert {:error, _} = CommandRouter.parse_command_subject("nats.myapp.cmd.counter.id1")
    end

    test "rejects subject without id" do
      assert {:error, _} = CommandRouter.parse_command_subject("quanta.myapp.cmd.counter")
    end

    test "rejects subject with wrong verb" do
      assert {:error, _} = CommandRouter.parse_command_subject("quanta.myapp.evt.counter.id1")
    end

    test "rejects subject with invalid namespace characters" do
      assert {:error, _} = CommandRouter.parse_command_subject("quanta.my app.cmd.counter.id1")
    end
  end

  # Helper to start an actor directly (bypassing the router)
  defp start_actor(actor_id, module \\ Counter) do
    opts = [actor_id: actor_id, module: module]
    DynSup.start_actor(actor_id, child_spec: {Server, opts})
  end
end
