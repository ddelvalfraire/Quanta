defmodule Quanta.Actor.EffectExecutorTest do
  use ExUnit.Case, async: false

  import ExUnit.CaptureLog

  alias Quanta.Actor.{DynSup, EffectExecutor, ManifestRegistry, Registry, Server}
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

  defp make_context(overrides \\ []) do
    actor_id = Keyword.get(overrides, :actor_id, make_actor_id("executor-test"))
    manifest = Keyword.get(overrides, :manifest, build_manifest())
    envelope = Keyword.get(overrides, :envelope, Envelope.new(payload: "test"))

    server_state =
      Keyword.get_lazy(overrides, :server_state, fn ->
        %Server{
          actor_id: actor_id,
          module: Counter,
          manifest: manifest,
          state_data: <<0::64>>,
          status: :active,
          activated_at: System.monotonic_time(),
          named_timers: %{},
          pending_replies: %{},
          message_count: 0
        }
      end)

    %{
      actor_id: actor_id,
      envelope: envelope,
      manifest: manifest,
      server_state: server_state
    }
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
    :ok
  end

  describe "empty effects" do
    test "returns initial result" do
      ctx = make_context()
      result = EffectExecutor.execute([], ctx)

      assert result.reply == nil
      assert result.server_state == ctx.server_state
      assert result.stop_self == false
      assert result.sent_ids == []
    end
  end

  describe ":reply" do
    test "single reply sets result" do
      ctx = make_context()
      result = EffectExecutor.execute([{:reply, "hello"}], ctx)

      assert result.reply == {:ok, "hello"}
    end

    test "multiple replies keeps first, warns on rest" do
      ctx = make_context()

      log =
        capture_log(fn ->
          result = EffectExecutor.execute([{:reply, "first"}, {:reply, "second"}], ctx)
          assert result.reply == {:ok, "first"}
        end)

      assert log =~ "Multiple :reply effects"
    end
  end

  describe ":persist" do
    test "success updates state_data and increments events_since_snapshot" do
      ctx = make_context()
      result = EffectExecutor.execute([{:persist, <<42::64>>}], ctx)

      assert result.server_state.state_data == <<42::64>>
      assert result.server_state.events_since_snapshot == 1
    end

    test "size exceeded returns persist_failed error" do
      manifest = build_manifest(state: %Manifest.State{max_size_bytes: 4})
      ctx = make_context(manifest: manifest)

      result = EffectExecutor.execute([{:persist, "too_large_data"}], ctx)

      assert {:error, :persist_failed, :state_too_large} = result
    end

    test "failure halts remaining effects" do
      manifest = build_manifest(state: %Manifest.State{max_size_bytes: 4})
      ctx = make_context(manifest: manifest)

      effects = [{:persist, "too_large_data"}, {:reply, "should_not_reach"}]
      result = EffectExecutor.execute(effects, ctx)

      assert {:error, :persist_failed, :state_too_large} = result
    end
  end

  describe ":set_timer" do
    test "creates timer and stores in named_timers" do
      ctx = make_context()
      result = EffectExecutor.execute([{:set_timer, "tick", 5_000}], ctx)

      assert Map.has_key?(result.server_state.named_timers, "tick")
      entry = result.server_state.named_timers["tick"]
      assert is_reference(entry.ref)
      assert entry.created_by == ctx.envelope.message_id

      Process.cancel_timer(entry.ref)
    end

    test "invalid delay (non-positive) is no-op" do
      ctx = make_context()

      result = EffectExecutor.execute([{:set_timer, "tick", 0}], ctx)
      assert result.server_state.named_timers == %{}

      result = EffectExecutor.execute([{:set_timer, "tick", -5}], ctx)
      assert result.server_state.named_timers == %{}
    end

    test "max timers limit drops timer" do
      resources = %Manifest.Resources{fuel_limit: 1_000_000, memory_limit_mb: 16, max_timers: 1}
      manifest = build_manifest(resources: resources)
      ctx = make_context(manifest: manifest)

      result = EffectExecutor.execute([{:set_timer, "t1", 10_000}], ctx)
      assert Map.has_key?(result.server_state.named_timers, "t1")

      ctx2 = %{ctx | server_state: result.server_state}
      result2 = EffectExecutor.execute([{:set_timer, "t2", 10_000}], ctx2)
      assert Map.has_key?(result2.server_state.named_timers, "t1")
      refute Map.has_key?(result2.server_state.named_timers, "t2")

      Process.cancel_timer(result2.server_state.named_timers["t1"].ref)
    end
  end

  describe ":cancel_timer" do
    test "removes existing timer" do
      ref = Process.send_after(self(), {:timer_fire, "tick"}, 10_000)
      entry = %{ref: ref, created_by: "test"}

      ctx = make_context()
      server_state = %{ctx.server_state | named_timers: %{"tick" => entry}}
      ctx = %{ctx | server_state: server_state}

      result = EffectExecutor.execute([{:cancel_timer, "tick"}], ctx)
      assert result.server_state.named_timers == %{}
    end

    test "no-op for missing timer" do
      ctx = make_context()
      result = EffectExecutor.execute([{:cancel_timer, "nonexistent"}], ctx)
      assert result.server_state.named_timers == %{}
    end
  end

  describe ":emit_telemetry" do
    test "fires telemetry event" do
      test_pid = self()

      :telemetry.attach(
        "executor-test-telemetry",
        [:quanta, :actor, :custom, :test_event],
        fn event, measurements, metadata, _config ->
          send(test_pid, {:telemetry, event, measurements, metadata})
        end,
        nil
      )

      ctx = make_context()

      EffectExecutor.execute(
        [{:emit_telemetry, "test_event", %{value: 1}, %{actor: "test"}}],
        ctx
      )

      assert_receive {:telemetry, [:quanta, :actor, :custom, :test_event], %{value: 1},
                       %{actor: "test"}}
    after
      :telemetry.detach("executor-test-telemetry")
    end
  end

  describe ":spawn_actor" do
    test "starts new actor via DynSup" do
      ctx = make_context()
      target = make_actor_id("spawned-by-executor")

      EffectExecutor.execute([{:spawn_actor, target, <<>>}], ctx)
      Process.sleep(50)

      assert {:ok, pid} = Registry.lookup(target)
      assert {:ok, <<0::64>>} = Server.get_state(pid)
    end
  end

  describe ":stop_self" do
    test "sets flag" do
      ctx = make_context()
      result = EffectExecutor.execute([:stop_self], ctx)
      assert result.stop_self == true
    end
  end

  describe ":side_effect" do
    test "runs via TaskSupervisor" do
      test_pid = self()
      ctx = make_context()

      EffectExecutor.execute(
        [{:side_effect, {Kernel, :send, [test_pid, :side_effect_ran]}}],
        ctx
      )

      assert_receive :side_effect_ran, 500
    end
  end

  describe ":send" do
    test "to local actor delivers via cast" do
      target_id = make_actor_id("send-target-exec")
      {:ok, _pid} = start_actor(target_id)

      ctx = make_context(actor_id: make_actor_id("send-source-exec"))
      result = EffectExecutor.execute([{:send, target_id, "inc"}], ctx)

      assert [_msg_id] = result.sent_ids
      Process.sleep(50)

      {:ok, pid} = Registry.lookup(target_id)
      assert {:ok, <<1::64>>} = Server.get_state(pid)
    end
  end

  describe "failure modes" do
    test ":side_effect MFA that raises does not crash executor" do
      ctx = make_context()

      result =
        EffectExecutor.execute(
          [{:side_effect, {Kernel, :raise, ["boom"]}}, {:reply, "ok"}],
          ctx
        )

      assert result.reply == {:ok, "ok"}
      Process.sleep(50)
    end

    test ":emit_telemetry with unknown atom segment drops event without crash" do
      ctx = make_context()

      log =
        capture_log(fn ->
          result =
            EffectExecutor.execute(
              [{:emit_telemetry, "nonexistent_xyz_atom_#{System.unique_integer()}", %{}, %{}}],
              ctx
            )

          assert result.reply == nil
        end)

      assert log =~ "Unknown telemetry event segment"
    end
  end

  describe "effect ordering" do
    test "effects processed in order" do
      ctx = make_context()

      effects = [
        {:persist, <<1::64>>},
        {:persist, <<2::64>>},
        {:reply, <<2::64>>}
      ]

      result = EffectExecutor.execute(effects, ctx)
      assert result.server_state.state_data == <<2::64>>
      assert result.server_state.events_since_snapshot == 2
      assert result.reply == {:ok, <<2::64>>}
    end
  end

  describe "NATS integration" do
    @describetag :nats

    test ":send to unknown actor publishes to NATS command subject" do
      target = make_actor_id("nats-send-target")
      subject = "quanta.#{target.namespace}.cmd.#{target.type}.#{target.id}"

      {:ok, _sid} = Gnat.sub(Quanta.Nats.Core.connection(0), self(), subject)

      ctx = make_context()
      EffectExecutor.execute([{:send, target, "hello"}], ctx)

      assert_receive {:msg, %{topic: ^subject, body: body}}, 1_000
      assert {:ok, envelope} = Quanta.Codec.Wire.decode(body)
      assert envelope.payload == "hello"
    end

    test ":publish publishes to NATS pub subject" do
      ctx = make_context()
      channel = "test-channel"
      subject = "quanta.#{ctx.actor_id.namespace}.pub.#{channel}"

      {:ok, _sid} = Gnat.sub(Quanta.Nats.Core.connection(0), self(), subject)

      EffectExecutor.execute([{:publish, channel, "pub_payload"}], ctx)

      assert_receive {:msg, %{topic: ^subject, body: "pub_payload"}}, 1_000
    end
  end

  describe ":log" do
    test "logs message and returns accumulator unchanged" do
      ctx = make_context()

      log_output =
        capture_log(fn ->
          result = EffectExecutor.execute([{:log, "test log message"}], ctx)
          assert result.reply == nil
          assert result.server_state == ctx.server_state
          assert result.stop_self == false
          assert result.sent_ids == []
        end)

      assert log_output =~ "test log message"
    end

    test "handles multiple log effects" do
      ctx = make_context()

      log_output =
        capture_log(fn ->
          result =
            EffectExecutor.execute(
              [{:log, "first message"}, {:log, "second message"}],
              ctx
            )

          assert result.reply == nil
          assert result.stop_self == false
        end)

      assert log_output =~ "first message"
      assert log_output =~ "second message"
    end
  end
end
