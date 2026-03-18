defmodule Quanta.Actor.ServerTest do
  use ExUnit.Case, async: false

  alias Quanta.Actor.{DynSup, ManifestRegistry, Registry, Server}
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

  defp make_envelope(payload) do
    Envelope.new(payload: payload)
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

  setup do
    clear_dynsup()

    :sys.replace_state(ManifestRegistry, fn state ->
      :ets.delete_all_objects(state)
      state
    end)

    :ok = ManifestRegistry.put(build_manifest())
    :ok = ManifestRegistry.put(build_manifest(type: "echo"))

    :ok
  end

  describe "activation" do
    test "actor activates via init path and get_state returns init state" do
      actor_id = make_actor_id("act-1")
      {:ok, pid} = start_actor(actor_id)
      assert Process.alive?(pid)
      assert {:ok, <<0::64>>} = Server.get_state(pid)
    end

    test "process_flag(:message_queue_data, :off_heap) is set" do
      actor_id = make_actor_id("act-offheap")
      {:ok, pid} = start_actor(actor_id)
      {:message_queue_data, mqd} = Process.info(pid, :message_queue_data)
      assert mqd == :off_heap
    end

    test "actor registers in Syn on activation" do
      actor_id = make_actor_id("act-syn")
      {:ok, pid} = start_actor(actor_id)
      assert {:ok, ^pid} = Registry.lookup(actor_id)
    end
  end

  describe "send_message/3" do
    test "returns {:ok, reply} when :reply effect present" do
      actor_id = make_actor_id("msg-reply")
      {:ok, pid} = start_actor(actor_id)
      envelope = make_envelope("get")
      assert {:ok, <<0::64>>} = Server.send_message(pid, envelope)
    end

    test "returns {:ok, :no_reply} when no :reply effect" do
      actor_id = make_actor_id("msg-noreply")
      {:ok, pid} = start_actor(actor_id)
      envelope = make_envelope("no_reply")
      assert {:ok, :no_reply} = Server.send_message(pid, envelope)
    end

    test ":persist effect updates state" do
      actor_id = make_actor_id("msg-persist")
      {:ok, pid} = start_actor(actor_id)

      envelope = make_envelope("inc")
      assert {:ok, <<1::64>>} = Server.send_message(pid, envelope)
      assert {:ok, <<1::64>>} = Server.get_state(pid)

      assert {:ok, <<2::64>>} = Server.send_message(pid, make_envelope("inc"))
      assert {:ok, <<2::64>>} = Server.get_state(pid)
    end
  end

  describe "get_meta/1" do
    test "returns metadata map" do
      actor_id = make_actor_id("meta-1")
      {:ok, pid} = start_actor(actor_id)

      assert {:ok, meta} = Server.get_meta(pid)
      assert meta.actor_id == actor_id
      assert meta.status == :active
      assert meta.message_count == 0
      assert is_integer(meta.activated_at)

      Server.send_message(pid, make_envelope("inc"))
      assert {:ok, meta} = Server.get_meta(pid)
      assert meta.message_count == 1
    end
  end

  describe "force_passivate/1" do
    test "stops the process normally" do
      actor_id = make_actor_id("passivate-force")
      {:ok, pid} = start_actor(actor_id)

      ref = Process.monitor(pid)
      assert :ok = Server.force_passivate(pid)
      assert_receive {:DOWN, ^ref, :process, ^pid, :normal}
    end

    test "deregisters from Syn on passivation" do
      actor_id = make_actor_id("passivate-dereg")
      {:ok, pid} = start_actor(actor_id)
      assert {:ok, ^pid} = Registry.lookup(actor_id)

      ref = Process.monitor(pid)
      Server.force_passivate(pid)
      assert_receive {:DOWN, ^ref, :process, ^pid, :normal}

      assert :not_found = Registry.lookup(actor_id)
    end
  end

  describe "idle timeout" do
    test "fires passivation after timeout" do
      short_lifecycle = %Manifest.Lifecycle{
        idle_timeout_ms: 50,
        idle_no_subscribers_timeout_ms: 30_000,
        max_concurrent_messages: 1,
        inter_actor_timeout_ms: 30_000,
        http_timeout_ms: 5_000
      }

      :ok = ManifestRegistry.put(build_manifest(type: "short", lifecycle: short_lifecycle))

      actor_id = make_actor_id("idle-1", "short")
      {:ok, pid} = start_actor(actor_id)
      ref = Process.monitor(pid)

      assert_receive {:DOWN, ^ref, :process, ^pid, :normal}, 500
      assert :not_found = Registry.lookup(actor_id)
    end

    test "message resets idle timer" do
      short_lifecycle = %Manifest.Lifecycle{
        idle_timeout_ms: 100,
        idle_no_subscribers_timeout_ms: 30_000,
        max_concurrent_messages: 1,
        inter_actor_timeout_ms: 30_000,
        http_timeout_ms: 5_000
      }

      :ok = ManifestRegistry.put(build_manifest(type: "short2", lifecycle: short_lifecycle))

      actor_id = make_actor_id("idle-2", "short2")
      {:ok, pid} = start_actor(actor_id)
      ref = Process.monitor(pid)

      Process.sleep(60)
      Server.send_message(pid, make_envelope("get"))
      Process.sleep(60)
      Server.send_message(pid, make_envelope("get"))

      assert Process.alive?(pid)

      assert_receive {:DOWN, ^ref, :process, ^pid, :normal}, 500
    end
  end

  describe ":send effect" do
    test "delivers message to local actor via cast" do
      actor_id_a = make_actor_id("send-a")
      actor_id_b = make_actor_id("send-b")
      {:ok, pid_a} = start_actor(actor_id_a)
      {:ok, pid_b} = start_actor(actor_id_b)

      envelope = Envelope.new(payload: "send:send-b")
      GenServer.cast(pid_a, {:incoming_message, envelope})
      Process.sleep(100)

      assert {:ok, <<1::64>>} = Server.get_state(pid_b)
    end

    test "send to unknown actor attempts NATS publish without crashing" do
      # Short inter_actor_timeout so the test doesn't hang 30s when NATS is up
      # (publish succeeds → pending reply stashed → times out)
      short_lifecycle = %Manifest.Lifecycle{
        idle_timeout_ms: 300_000,
        idle_no_subscribers_timeout_ms: 30_000,
        max_concurrent_messages: 1,
        inter_actor_timeout_ms: 200,
        http_timeout_ms: 5_000
      }

      :ok = ManifestRegistry.put(build_manifest(type: "nats_send", lifecycle: short_lifecycle))

      actor_id = make_actor_id("send-unknown-src", "nats_send")
      {:ok, pid} = start_actor(actor_id)

      result = Server.send_message(pid, make_envelope("send:nonexistent"), 5_000)
      # Without NATS: publish fails → {:ok, :no_reply} immediately
      # With NATS: publish succeeds → pending reply times out
      assert result in [{:ok, :no_reply}, {:error, :actor_timeout}]
      assert Process.alive?(pid)
    end
  end

  describe ":set_timer / :cancel_timer effects" do
    test "set_timer creates timer that fires after delay" do
      actor_id = make_actor_id("timer-set")
      {:ok, pid} = start_actor(actor_id)

      Server.send_message(pid, make_envelope("set_timer:tick:50"))
      Process.sleep(100)

      assert {:ok, <<10::64>>} = Server.get_state(pid)
    end

    test "cancel_timer cancels existing timer" do
      actor_id = make_actor_id("timer-cancel")
      {:ok, pid} = start_actor(actor_id)

      Server.send_message(pid, make_envelope("set_timer:tick:200"))
      Process.sleep(50)
      Server.send_message(pid, make_envelope("cancel_timer:tick"))
      Process.sleep(250)

      assert {:ok, <<0::64>>} = Server.get_state(pid)
    end

    test "cancel_timer for missing timer is no-op" do
      actor_id = make_actor_id("timer-cancel-noop")
      {:ok, pid} = start_actor(actor_id)

      assert {:ok, _} = Server.send_message(pid, make_envelope("cancel_timer:nonexistent"))
      assert Process.alive?(pid)
    end

    test "timer fire calls handle_timer and updates state" do
      actor_id = make_actor_id("timer-fire")
      {:ok, pid} = start_actor(actor_id)

      Server.send_message(pid, make_envelope("set_timer:tick:30"))
      Process.sleep(80)

      assert {:ok, <<10::64>>} = Server.get_state(pid)
    end

    test "max timers limit enforced" do
      manifest =
        build_manifest(
          type: "limited",
          resources: %Manifest.Resources{
            fuel_limit: 1_000_000,
            memory_limit_mb: 16,
            max_timers: 2
          }
        )

      :ok = ManifestRegistry.put(manifest)

      actor_id = make_actor_id("timer-max", "limited")
      {:ok, pid} = start_actor(actor_id)

      Server.send_message(pid, make_envelope("set_timer:t1:10000"))
      Server.send_message(pid, make_envelope("set_timer:t2:10000"))
      Server.send_message(pid, make_envelope("set_timer:t3:10000"))

      assert Process.alive?(pid)
    end
  end

  describe ":spawn_actor effect" do
    test "starts new actor via DynSup" do
      actor_id = make_actor_id("spawner-1")
      {:ok, pid} = start_actor(actor_id)

      Server.send_message(pid, make_envelope("spawn:spawned-1"))
      Process.sleep(50)

      spawned_id = make_actor_id("spawned-1")
      assert {:ok, spawned_pid} = Registry.lookup(spawned_id)
      assert {:ok, <<0::64>>} = Server.get_state(spawned_pid)
    end
  end

  describe ":stop_self effect" do
    test "stops actor normally" do
      actor_id = make_actor_id("stop-self")
      {:ok, pid} = start_actor(actor_id)
      ref = Process.monitor(pid)

      Server.send_message(pid, make_envelope("stop"))
      assert_receive {:DOWN, ^ref, :process, ^pid, :normal}, 500
    end
  end

  describe ":side_effect effect" do
    test "runs without crashing actor" do
      actor_id = make_actor_id("side-effect-1")
      {:ok, pid} = start_actor(actor_id)

      Server.send_message(pid, make_envelope("side_effect"))
      Process.sleep(50)
      assert Process.alive?(pid)
    end
  end

  describe ":emit_telemetry effect" do
    test "executes telemetry event" do
      actor_id = make_actor_id("telemetry-1")
      {:ok, pid} = start_actor(actor_id)

      test_pid = self()

      :telemetry.attach(
        "test-custom-telemetry",
        [:quanta, :actor, :custom, :test_event],
        fn event, measurements, metadata, _config ->
          send(test_pid, {:telemetry, event, measurements, metadata})
        end,
        nil
      )

      Server.send_message(pid, make_envelope("telemetry"))
      assert_receive {:telemetry, [:quanta, :actor, :custom, :test_event], %{value: 1}, %{actor: "counter"}}
    after
      :telemetry.detach("test-custom-telemetry")
    end
  end

  describe ":publish effect" do
    test "publishes to NATS without crashing" do
      actor_id = make_actor_id("publish-1")
      {:ok, pid} = start_actor(actor_id)

      assert {:ok, :no_reply} = Server.send_message(pid, make_envelope("publish:test-channel"))
      assert Process.alive?(pid)
    end
  end

  describe "incoming cast" do
    test "handle_cast processes message without reply" do
      actor_id = make_actor_id("cast-1")
      {:ok, pid} = start_actor(actor_id)

      envelope = Envelope.new(payload: "inc", sender: :system)
      GenServer.cast(pid, {:incoming_message, envelope})
      Process.sleep(50)

      assert {:ok, <<1::64>>} = Server.get_state(pid)
    end
  end

  describe "mailbox shedding" do
    test "returns {:error, :overloaded} above shed threshold" do
      actor_id = make_actor_id("mailbox-shed")
      {:ok, pid} = start_actor(actor_id)

      :erlang.suspend_process(pid)

      task = Task.async(fn -> Server.send_message(pid, make_envelope("get"), 5_000) end)
      Process.sleep(10)

      for _ <- 1..5_500 do
        send(pid, :dummy)
      end

      :erlang.resume_process(pid)
      result = Task.await(task, 10_000)
      assert {:error, :overloaded} = result
    end
  end

  describe "idle timer race" do
    test "stale passivate message does not kill active actor" do
      short_lifecycle = %Manifest.Lifecycle{
        idle_timeout_ms: 30,
        idle_no_subscribers_timeout_ms: 30_000,
        max_concurrent_messages: 1,
        inter_actor_timeout_ms: 30_000,
        http_timeout_ms: 5_000
      }

      :ok = ManifestRegistry.put(build_manifest(type: "race", lifecycle: short_lifecycle))

      actor_id = make_actor_id("race-1", "race")
      {:ok, pid} = start_actor(actor_id)

      for _ <- 1..20 do
        Server.send_message(pid, make_envelope("get"))
        Process.sleep(5)
      end

      assert Process.alive?(pid)
      assert {:ok, <<0::64>>} = Server.get_state(pid)
    end
  end

  describe "pending replies" do
    test "caller gets reply when target actor responds" do
      :ok = ManifestRegistry.put(build_manifest(type: "responder"))

      responder_id = make_actor_id("pr-req", "responder")
      counter_id = make_actor_id("pr-counter")
      {:ok, responder_pid} = start_actor(responder_id, Quanta.Test.Actors.Responder)
      {:ok, _} = start_actor(counter_id)

      result = Server.send_message(responder_pid, make_envelope("ask:pr-counter"), 5_000)
      assert {:ok, "pong"} = result
    end

    test "pending reply times out with {:error, :actor_timeout}" do
      short_lifecycle = %Manifest.Lifecycle{
        idle_timeout_ms: 300_000,
        idle_no_subscribers_timeout_ms: 30_000,
        max_concurrent_messages: 1,
        inter_actor_timeout_ms: 100,
        http_timeout_ms: 5_000
      }

      :ok = ManifestRegistry.put(build_manifest(type: "fast_timeout", lifecycle: short_lifecycle))

      sender_id = make_actor_id("pr-timeout-sender", "fast_timeout")
      target_id = make_actor_id("pr-timeout-target")
      {:ok, sender_pid} = start_actor(sender_id)
      {:ok, _} = start_actor(target_id)

      result = Server.send_message(sender_pid, make_envelope("send:pr-timeout-target"), 5_000)
      assert {:error, :actor_timeout} = result
    end
  end

  describe "init failure tracking" do
    test "3 consecutive init failures stops with :normal" do
      Process.flag(:trap_exit, true)
      :ok = ManifestRegistry.put(build_manifest(type: "failer"))

      actor_id = %ActorId{namespace: "test", type: "failer", id: "fail-3x"}
      opts = [actor_id: actor_id, module: Quanta.Test.Actors.Failer]

      for i <- 1..3 do
        {:ok, pid} = Server.start_link(opts)
        assert_receive {:EXIT, ^pid, reason}, 1000

        if i == 3, do: assert(reason == :normal)

        Process.sleep(50)
      end
    after
      Process.flag(:trap_exit, false)
    end
  end

  describe "DynSup default child_spec" do
    test "start_actor without explicit child_spec starts Actor.Server" do
      actor_id = make_actor_id("dynsup-default")
      {:ok, pid} = DynSup.start_actor(actor_id, module: Counter)

      assert Process.alive?(pid)
      assert {:ok, <<0::64>>} = Server.get_state(pid)
      assert {:ok, ^pid} = Registry.lookup(actor_id)
    end
  end

  describe "telemetry atom safety" do
    test "unknown telemetry event atom is dropped without crash" do
      actor_id = make_actor_id("telemetry-safe")
      {:ok, pid} = start_actor(actor_id)

      Server.send_message(pid, make_envelope("telemetry"))
      assert Process.alive?(pid)
    end
  end
end
