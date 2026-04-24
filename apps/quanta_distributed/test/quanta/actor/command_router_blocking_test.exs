defmodule Quanta.Actor.CommandRouterBlockingTest do
  # FINDING 3 (CRITICAL-1): NATS ingress blocks on slow actor
  #
  # CommandRouter.handle_info/2 for {:msg, ...} calls route/3 inline, which
  # blocks on the actor's GenServer.call for up to 30 s (command_router.ex
  # lines 315-330).  A single slow actor stalls the entire NATS ingress loop
  # because the CommandRouter GenServer cannot process the next message until
  # the current route/3 call returns.
  #
  # This test reproduces the bug WITHOUT a real NATS connection by sending
  # {:msg, ...} directly to the CommandRouter process (same message shape
  # NATS delivers).  Two mock actors are pre-registered:
  #   A — slow: handle_message sleeps 500 ms before replying.
  #   B — fast: handle_message replies immediately.
  #
  # We fire the message for A first, then immediately fire the message for B.
  # We then probe the CommandRouter with a synchronous call to measure how long
  # it stays blocked.
  #
  # EXPECTED FAILURE TODAY: router_blocked_ms ≈ 500 ms (blocked behind A),
  # so the assertion `< 100 ms` fails.
  #
  # Do NOT fix the underlying code.  This is a RED test that documents the bug.

  use ExUnit.Case, async: false

  alias Quanta.Actor.{DynSup, ManifestRegistry, Registry, Server}
  alias Quanta.{ActorId, Envelope, Manifest}

  @namespace "test"

  # ---------------------------------------------------------------------------
  # Slow actor module — sleeps 500 ms in handle_message to simulate a stall
  # ---------------------------------------------------------------------------

  defmodule SlowActor do
    @moduledoc false
    @behaviour Quanta.Actor

    @impl true
    def init(_payload), do: {"idle", []}

    @impl true
    def handle_message(state, _envelope) do
      Process.sleep(500)
      {state, [{:reply, "slow_done"}]}
    end

    @impl true
    def handle_timer(state, _), do: {state, []}

    @impl true
    def on_passivate(state), do: state
  end

  # ---------------------------------------------------------------------------
  # Fast actor module — replies immediately
  # ---------------------------------------------------------------------------

  defmodule FastActor do
    @moduledoc false
    @behaviour Quanta.Actor

    @impl true
    def init(_payload), do: {"idle", []}

    @impl true
    def handle_message(state, _envelope) do
      {state, [{:reply, "fast_done"}]}
    end

    @impl true
    def handle_timer(state, _), do: {state, []}

    @impl true
    def on_passivate(state), do: state
  end

  defp build_manifest(type) do
    struct!(Manifest, version: "1", type: type, namespace: @namespace)
  end

  defp make_actor_id(type, id) do
    %ActorId{namespace: @namespace, type: type, id: id}
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

    :sys.replace_state(ManifestRegistry, fn state ->
      :ets.delete_all_objects(state)
      state
    end)

    :ok = ManifestRegistry.put(build_manifest("slow_actor"))
    :ok = ManifestRegistry.put(build_manifest("fast_actor"))

    prev_modules = Application.get_env(:quanta_distributed, :actor_modules, %{})

    Application.put_env(:quanta_distributed, :actor_modules, %{
      {@namespace, "slow_actor"} => SlowActor,
      {@namespace, "fast_actor"} => FastActor
    })

    on_exit(fn ->
      Application.put_env(:quanta_distributed, :actor_modules, prev_modules)
    end)

    :ok
  end

  # ---------------------------------------------------------------------------
  # Build the {:msg, ...} map that NATS delivers to CommandRouter.handle_info
  # ---------------------------------------------------------------------------

  defp nats_msg(actor_id, payload) do
    topic = "quanta.#{actor_id.namespace}.cmd.#{actor_id.type}.#{actor_id.id}"
    envelope = Envelope.new(payload: payload, sender: {:client, "nats"})
    body = Quanta.Codec.Wire.encode(envelope)
    %{topic: topic, body: body, reply_to: nil}
  end

  # ---------------------------------------------------------------------------
  # Main test
  # ---------------------------------------------------------------------------

  describe "CommandRouter.handle_info/2 — blocking ingress" do
    test "router remains responsive while slow actor A is processing (B not blocked)" do
      slow_id = make_actor_id("slow_actor", "blocking-slow-#{System.unique_integer()}")
      fast_id = make_actor_id("fast_actor", "blocking-fast-#{System.unique_integer()}")

      # Pre-start both actors so routing goes straight to deliver_direct/2 and
      # the manifest/rate-limit overhead does not skew the timing measurement.
      {:ok, _} =
        DynSup.start_actor(slow_id,
          child_spec: {Server, [actor_id: slow_id, module: SlowActor]}
        )

      {:ok, _} =
        DynSup.start_actor(fast_id,
          child_spec: {Server, [actor_id: fast_id, module: FastActor]}
        )

      assert {:ok, _} = Registry.lookup(slow_id)
      assert {:ok, _} = Registry.lookup(fast_id)

      router_pid = Process.whereis(Quanta.Actor.CommandRouter)
      assert is_pid(router_pid), "CommandRouter is not running — cannot run this test"

      # Fire message for slow actor A into the router's mailbox.
      # handle_info will pick it up and block synchronously for ~500 ms.
      send(router_pid, {:msg, nats_msg(slow_id, "ping")})

      # Immediately fire message for fast actor B.
      # With the bug: this message sits in the router's queue behind A.
      send(router_pid, {:msg, nats_msg(fast_id, "ping")})

      # Give the router a moment to start processing A's message (so it is
      # definitely inside the blocking route/3 call before we probe it).
      Process.sleep(20)

      # Probe: how long does a synchronous call to the router take right now?
      # With the bug: the router is stuck on A's 500 ms sleep, so :unsubscribe
      # queues behind both A's handle_info and B's handle_info — meaning our
      # call waits ~480 ms (remainder of A) + 0 ms (B fast) ≈ 480 ms total.
      # Fixed code: the router spawned A's work into a Task so it is free;
      # our call returns in < 5 ms.
      t0 = System.monotonic_time(:millisecond)

      try do
        GenServer.call(router_pid, :unsubscribe, 2_000)
      catch
        :exit, _ -> :ok
      end

      router_blocked_ms = System.monotonic_time(:millisecond) - t0

      # BUG: today router_blocked_ms ≈ 480 ms, so this assertion fails.
      # Fixed code dispatches route/3 into a Task pool; router_blocked_ms < 100 ms.
      assert router_blocked_ms < 100,
             "CommandRouter was blocked for #{router_blocked_ms} ms — " <>
               "handle_info dispatches route/3 synchronously, stalling all NATS ingress " <>
               "(CRITICAL-1). Expected < 100 ms when non-blocking."
    end
  end
end
