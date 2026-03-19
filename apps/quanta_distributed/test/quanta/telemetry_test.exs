defmodule Quanta.TelemetryTest do
  use ExUnit.Case, async: false

  alias Quanta.Actor.{CommandRouter, DynSup, ManifestRegistry, Server}
  alias Quanta.{ActorId, Envelope, Manifest}
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

  defp make_actor_id(id, type \\ @type_name) do
    %ActorId{namespace: @namespace, type: type, id: id}
  end

  defp start_actor(actor_id, module \\ Counter) do
    opts = [actor_id: actor_id, module: module]
    DynSup.start_actor(actor_id, child_spec: {Server, opts})
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

  defp attach(handler_id, events) do
    test_pid = self()

    :telemetry.attach_many(
      handler_id,
      events,
      fn event, measurements, metadata, _config ->
        send(test_pid, {:telemetry, event, measurements, metadata})
      end,
      nil
    )
  end

  setup do
    clear_dynsup()

    :sys.replace_state(ManifestRegistry, fn state ->
      :ets.delete_all_objects(state)
      state
    end)

    # Ensure clean RateLimit ETS
    if :ets.whereis(Quanta.RateLimit) != :undefined do
      :ets.delete(Quanta.RateLimit)
    end

    Quanta.RateLimit.init()

    :ok = ManifestRegistry.put(build_manifest())
    :ok = ManifestRegistry.put(build_manifest(type: "echo"))
    :ok = ManifestRegistry.put(build_manifest(type: "failer"))

    :ok
  end

  describe "message dispatch span" do
    @tag :telemetry
    test "emits start and stop events" do
      attach("msg-span", [
        [:quanta, :actor, :message, :start],
        [:quanta, :actor, :message, :stop]
      ])

      actor_id = make_actor_id("tel-msg-1")
      {:ok, pid} = start_actor(actor_id)
      Server.send_message(pid, Envelope.new(payload: "get"))

      assert_receive {:telemetry, [:quanta, :actor, :message, :start], %{system_time: _},
                       %{actor_id: ^actor_id}}

      assert_receive {:telemetry, [:quanta, :actor, :message, :stop], %{duration: _},
                       %{actor_id: ^actor_id}}
    after
      :telemetry.detach("msg-span")
    end
  end

  describe "activation span" do
    @tag :telemetry
    test "emits start and stop events" do
      attach("activate-span", [
        [:quanta, :actor, :activate, :start],
        [:quanta, :actor, :activate, :stop]
      ])

      actor_id = make_actor_id("tel-activate-1")
      {:ok, _pid} = start_actor(actor_id)

      assert_receive {:telemetry, [:quanta, :actor, :activate, :start], %{system_time: _},
                       %{actor_id: ^actor_id}}

      assert_receive {:telemetry, [:quanta, :actor, :activate, :stop], %{duration: _},
                       %{actor_id: ^actor_id}}
    after
      :telemetry.detach("activate-span")
    end
  end

  describe "passivate event" do
    @tag :telemetry
    test "emits on idle passivation" do
      attach("passivate-idle", [[:quanta, :actor, :passivate]])

      short_lifecycle = %Manifest.Lifecycle{
        idle_timeout_ms: 30,
        idle_no_subscribers_timeout_ms: 30,
        max_concurrent_messages: 1,
        inter_actor_timeout_ms: 30_000,
        http_timeout_ms: 5_000
      }

      :ok = ManifestRegistry.put(build_manifest(type: "short", lifecycle: short_lifecycle))

      actor_id = make_actor_id("tel-passivate-1", "short")
      {:ok, pid} = start_actor(actor_id)
      ref = Process.monitor(pid)

      assert_receive {:DOWN, ^ref, :process, ^pid, :normal}, 500

      assert_receive {:telemetry, [:quanta, :actor, :passivate], %{},
                       %{actor_id: ^actor_id, reason: :idle}}
    after
      :telemetry.detach("passivate-idle")
    end

    @tag :telemetry
    test "emits on force passivation" do
      attach("passivate-force", [[:quanta, :actor, :passivate]])

      actor_id = make_actor_id("tel-passivate-2")
      {:ok, pid} = start_actor(actor_id)
      Server.force_passivate(pid)

      assert_receive {:telemetry, [:quanta, :actor, :passivate], %{},
                       %{actor_id: ^actor_id, reason: :force}}
    after
      :telemetry.detach("passivate-force")
    end
  end

  describe "crash event" do
    @tag :telemetry
    test "emits on init failure" do
      Process.flag(:trap_exit, true)
      attach("crash-init", [[:quanta, :actor, :crash]])

      actor_id = %ActorId{namespace: "test", type: "failer", id: "tel-crash-1"}
      opts = [actor_id: actor_id, module: Quanta.Test.Actors.Failer]
      {:ok, pid} = Server.start_link(opts)
      assert_receive {:EXIT, ^pid, _}, 1000

      assert_receive {:telemetry, [:quanta, :actor, :crash], %{},
                       %{actor_id: ^actor_id, reason: _, stacktrace: _}}
    after
      Process.flag(:trap_exit, false)
      :telemetry.detach("crash-init")
    end
  end

  describe "rate_limit rejected event" do
    @tag :telemetry
    test "emits when rate limited" do
      attach("rate-rejected", [[:quanta, :rate_limit, :rejected]])

      # Set a very restrictive rate limit
      rl = %Manifest.RateLimits{messages_per_second: 1, messages_per_second_type: 100}
      :ok = ManifestRegistry.put(build_manifest(type: "limited", rate_limits: rl))

      actor_id = make_actor_id("tel-rl-1", "limited")
      {:ok, _pid} = start_actor(actor_id)

      # First call consumes the token
      CommandRouter.route(actor_id, Envelope.new(payload: "get"))
      # Second call should be rate limited
      assert {:error, :rate_limited} = CommandRouter.route(actor_id, Envelope.new(payload: "get"))

      assert_receive {:telemetry, [:quanta, :rate_limit, :rejected], %{}, %{actor_id: ^actor_id}}
    after
      :telemetry.detach("rate-rejected")
    end
  end

  describe "Logger metadata" do
    @tag :telemetry
    test "sets actor metadata on process" do
      actor_id = make_actor_id("tel-logger-1")
      {:ok, pid} = start_actor(actor_id)

      meta =
        :sys.get_state(pid)
        |> then(fn _state ->
          # Read Logger metadata from the actor process
          :rpc.call(node(), Process, :info, [pid, :dictionary])
        end)
        |> elem(1)
        |> Keyword.get(:"$logger_metadata$", %{})

      assert meta[:actor_namespace] == "test"
      assert meta[:actor_type] == "counter"
      assert meta[:actor_id] == "tel-logger-1"
    end
  end
end
