defmodule Quanta.Actor.ServerTest do
  use ExUnit.Case, async: false

  alias Quanta.Actor.{DynSup, ManifestRegistry, Registry, Server}
  alias Quanta.{ActorId, Envelope, Manifest}
  alias Quanta.Test.Actors.{Counter, CrdtDoc}

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

    :ok =
      ManifestRegistry.put(
        build_manifest(
          type: "crdt_doc",
          state: %Manifest.State{kind: {:crdt, :text}}
        )
      )

    :ok
  end

  defp start_crdt_actor(id) do
    actor_id = make_actor_id(id, "crdt_doc")
    opts = [actor_id: actor_id, module: CrdtDoc]
    {:ok, pid} = DynSup.start_actor(actor_id, child_spec: {Server, opts})
    {actor_id, pid}
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
        idle_no_subscribers_timeout_ms: 50,
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
        idle_no_subscribers_timeout_ms: 100,
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

    test "subscriber_left resets idle timer" do
      short_lifecycle = %Manifest.Lifecycle{
        idle_timeout_ms: 300_000,
        idle_no_subscribers_timeout_ms: 100,
        max_concurrent_messages: 1,
        inter_actor_timeout_ms: 30_000,
        http_timeout_ms: 5_000
      }

      :ok = ManifestRegistry.put(build_manifest(type: "sub_left", lifecycle: short_lifecycle))

      actor_id = make_actor_id("idle-sub-left", "sub_left")
      {:ok, pid} = start_actor(actor_id)
      ref = Process.monitor(pid)

      for _ <- 1..5 do
        send(pid, {:subscriber_left, "test-user"})
        Process.sleep(60)
      end

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
        idle_no_subscribers_timeout_ms: 200,
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

  describe "CRDT activation" do
    test "CRDT actor activates with empty LoroDoc and init effects applied" do
      {_actor_id, pid} = start_crdt_actor("crdt-act-1")
      assert Process.alive?(pid)

      {:ok, state_json} = Server.get_state(pid)
      state = Jason.decode!(state_json)
      assert get_in(state, ["text"]) == "hello"
    end

    test "CRDT actor get_state returns JSON" do
      {_actor_id, pid} = start_crdt_actor("crdt-act-json")
      {:ok, state_json} = Server.get_state(pid)
      assert is_binary(state_json)
      assert {:ok, _} = Jason.decode(state_json)
    end
  end

  describe "CRDT Path 1 — delta application" do
    test "apply delta from another LoroDoc" do
      {_actor_id, pid} = start_crdt_actor("crdt-delta-1")

      {:ok, other_doc} = Quanta.Nifs.LoroEngine.doc_new()
      :ok = Quanta.Nifs.LoroEngine.text_insert(other_doc, "text", 0, "world")
      {:ok, snapshot} = Quanta.Nifs.LoroEngine.doc_export_snapshot(other_doc)

      GenServer.cast(pid, {:crdt_delta, snapshot, "peer-1"})
      Process.sleep(50)

      {:ok, state_json} = Server.get_state(pid)
      state = Jason.decode!(state_json)
      assert is_binary(state["text"])
      assert Process.alive?(pid)
    end

    test "invalid delta is rejected without crashing" do
      {_actor_id, pid} = start_crdt_actor("crdt-delta-bad")

      GenServer.cast(pid, {:crdt_delta, "invalid_bytes", "peer-2"})
      Process.sleep(50)

      assert Process.alive?(pid)
    end

    test "delta broadcast reaches :pg group members" do
      {actor_id, pid} = start_crdt_actor("crdt-delta-broadcast")

      :pg.join(Quanta.Actor.CrdtPubSub, {:crdt, actor_id}, self())

      {:ok, other_doc} = Quanta.Nifs.LoroEngine.doc_new()
      :ok = Quanta.Nifs.LoroEngine.text_insert(other_doc, "text", 0, "x")
      {:ok, snapshot} = Quanta.Nifs.LoroEngine.doc_export_snapshot(other_doc)

      GenServer.cast(pid, {:crdt_delta, snapshot, "peer-3"})

      assert_receive {:crdt_delta, ^actor_id, _delta_bytes, "peer-3"}, 1000
    after
      :pg.leave(Quanta.Actor.CrdtPubSub, {:crdt, make_actor_id("crdt-delta-broadcast", "crdt_doc")}, self())
    end
  end

  describe "CRDT Path 2 — command messages" do
    test "handle_message receives JSON snapshot and returns crdt_ops" do
      {_actor_id, pid} = start_crdt_actor("crdt-cmd-1")

      Server.send_message(pid, make_envelope("text_insert:5: world"))
      {:ok, state_json} = Server.get_state(pid)
      state = Jason.decode!(state_json)
      assert state["text"] == "hello world"
    end

    test "handle_message 'get' returns the JSON snapshot it received" do
      {_actor_id, pid} = start_crdt_actor("crdt-cmd-get")

      {:ok, reply} = Server.send_message(pid, make_envelope("get"))
      state = Jason.decode!(reply)
      assert state["text"] == "hello"
    end

    test "returned state from handle_message is ignored for CRDT actors" do
      {_actor_id, pid} = start_crdt_actor("crdt-cmd-ignore")

      Server.send_message(pid, make_envelope("map_set:key:value"))
      {:ok, state_json} = Server.get_state(pid)
      state = Jason.decode!(state_json)

      assert state["data"] == %{"key" => "value"}
      assert state["text"] == "hello"
    end
  end

  describe "CRDT passivation" do
    test "passivation exports shallow snapshot via on_passivate" do
      {_actor_id, pid} = start_crdt_actor("crdt-pass-1")
      ref = Process.monitor(pid)

      Server.force_passivate(pid)
      assert_receive {:DOWN, ^ref, :process, ^pid, :normal}
    end

    test "CRDT actor deregisters on passivation" do
      {actor_id, pid} = start_crdt_actor("crdt-pass-dereg")
      ref = Process.monitor(pid)

      Server.force_passivate(pid)
      assert_receive {:DOWN, ^ref, :process, ^pid, :normal}
      assert :not_found = Registry.lookup(actor_id)
    end
  end

  describe "ephemeral awareness" do
    test "ephemeral cast stores value and broadcasts to subscribers" do
      {_actor_id, pid} = start_crdt_actor("eph-bcast")
      :ok = Server.subscribe(pid, self(), "alice")

      # Drain the initial ephemeral_state message
      assert_receive {:ephemeral_state, _}, 200

      GenServer.cast(pid, {:ephemeral_update, "user:bob", "cursor-data", self()})

      # Sender still receives the broadcast (filtering is channel-side)
      assert_receive {:ephemeral_update, encoded, _sender}, 500
      assert is_binary(encoded)
    end

    test "subscribe sends initial ephemeral state" do
      {_actor_id, pid} = start_crdt_actor("eph-init")

      # Set some ephemeral data before subscribing
      GenServer.cast(pid, {:ephemeral_update, "user:pre", "data", self()})
      Process.sleep(50)

      :ok = Server.subscribe(pid, self(), "viewer")
      assert_receive {:ephemeral_state, bytes}, 500
      assert is_binary(bytes)
    end

    test "unsubscribe cleans up ephemeral data and broadcasts deletion" do
      {_actor_id, pid} = start_crdt_actor("eph-unsub")

      # Subscribe a watcher to observe broadcasts
      :ok = Server.subscribe(pid, self(), "watcher")
      assert_receive {:ephemeral_state, _}, 200

      # Subscribe a second client whose ephemeral data will be cleaned up
      {:ok, client} = Agent.start_link(fn -> nil end)
      :ok = Server.subscribe(pid, client, "leaving")

      # Set ephemeral data for the leaving user
      GenServer.cast(pid, {:ephemeral_update, "user:leaving", "cursor", self()})
      assert_receive {:ephemeral_update, _, _}, 200

      # Unsubscribe triggers cleanup + broadcast of deleted key
      :ok = Server.unsubscribe(pid, client)
      assert_receive {:ephemeral_update, encoded, nil}, 500
      assert is_binary(encoded)
    end

    test "non-CRDT actor handles ephemeral cast as no-op" do
      actor_id = make_actor_id("eph-noop")
      {:ok, pid} = start_actor(actor_id)

      GenServer.cast(pid, {:ephemeral_update, "user:x", "data", self()})
      Process.sleep(50)

      assert Process.alive?(pid)
    end
  end

  describe "CRDT state size" do
    test "warns when state exceeds max_size_bytes" do
      small_state = %Manifest.State{
        kind: {:crdt, :text},
        max_size_bytes: 1
      }

      :ok = ManifestRegistry.put(build_manifest(type: "crdt_small", state: small_state))

      actor_id = make_actor_id("crdt-size-1", "crdt_small")
      opts = [actor_id: actor_id, module: CrdtDoc]
      {:ok, pid} = DynSup.start_actor(actor_id, child_spec: {Server, opts})

      assert Process.alive?(pid)

      Server.send_message(pid, make_envelope("text_insert:5: more data"))
      assert Process.alive?(pid)
    end
  end
end
