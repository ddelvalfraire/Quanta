defmodule Quanta.Actor.SynEventHandlerTest do
  use ExUnit.Case, async: false

  import ExUnit.CaptureLog

  alias Quanta.Actor.SynEventHandler
  alias Quanta.ActorId

  setup do
    :syn.add_node_to_scopes([:actors])
    :ok
  end

  defp make_actor_id(id) do
    %ActorId{namespace: "test", type: "counter", id: id}
  end

  defp spawn_waiting do
    spawn(fn ->
      receive do
        :stop -> :ok
      end
    end)
  end

  defp meta(overrides \\ %{}) do
    %{node: node(), type: "counter", nonce: :rand.uniform(0xFFFFFFFFFFFFFFFF),
      activated_at: System.monotonic_time(), draining: false}
    |> Map.merge(overrides)
  end

  describe "resolve_registry_conflict/4" do
    test "both non-draining — keeps older registration (lower syn time)" do
      actor_id = make_actor_id("conflict-older")
      pid1 = spawn_waiting()
      pid2 = spawn_waiting()

      log =
        capture_log(fn ->
          result =
            SynEventHandler.resolve_registry_conflict(
              :actors, actor_id,
              {pid1, meta(), 100},
              {pid2, meta(), 200}
            )

          assert result == pid1
        end)

      refute Process.alive?(pid2)
      assert log =~ "Registry conflict resolved"
    end

    test "one draining — keeps non-draining regardless of time" do
      actor_id = make_actor_id("conflict-drain")
      pid1 = spawn_waiting()
      pid2 = spawn_waiting()

      SynEventHandler.resolve_registry_conflict(
        :actors, actor_id,
        {pid1, meta(%{draining: true}), 100},
        {pid2, meta(%{draining: false}), 200}
      )

      refute Process.alive?(pid1)
      assert Process.alive?(pid2)
    end

    test "other side draining — keeps non-draining" do
      actor_id = make_actor_id("conflict-drain-2")
      pid1 = spawn_waiting()
      pid2 = spawn_waiting()

      result =
        SynEventHandler.resolve_registry_conflict(
          :actors, actor_id,
          {pid1, meta(%{draining: false}), 200},
          {pid2, meta(%{draining: true}), 100}
        )

      assert result == pid1
      refute Process.alive?(pid2)
    end

    test "both draining — keeps older registration" do
      actor_id = make_actor_id("conflict-both-drain")
      pid1 = spawn_waiting()
      pid2 = spawn_waiting()

      result =
        SynEventHandler.resolve_registry_conflict(
          :actors, actor_id,
          {pid1, meta(%{draining: true}), 100},
          {pid2, meta(%{draining: true}), 200}
        )

      assert result == pid1
      refute Process.alive?(pid2)
    end
  end

  describe "on_process_registered/5" do
    test "emits telemetry on :syn_conflict_resolution" do
      actor_id = make_actor_id("telemetry-conflict")
      test_pid = self()

      :telemetry.attach(
        "test-conflict-resolved",
        [:quanta, :actor, :conflict_resolved],
        fn event, measurements, metadata, _config ->
          send(test_pid, {:telemetry_event, event, measurements, metadata})
        end,
        nil
      )

      SynEventHandler.on_process_registered(
        :actors,
        actor_id,
        self(),
        %{draining: false},
        :syn_conflict_resolution
      )

      assert_receive {:telemetry_event, [:quanta, :actor, :conflict_resolved], %{},
                       %{actor_id: ^actor_id, pid: _, meta: %{draining: false}}}

      :telemetry.detach("test-conflict-resolved")
    end

    test "no-op for normal registration" do
      actor_id = make_actor_id("telemetry-normal")
      test_pid = self()

      :telemetry.attach(
        "test-no-conflict",
        [:quanta, :actor, :conflict_resolved],
        fn _event, _measurements, _metadata, _config ->
          send(test_pid, :unexpected_telemetry)
        end,
        nil
      )

      SynEventHandler.on_process_registered(:actors, actor_id, self(), %{}, :normal)

      refute_receive :unexpected_telemetry, 50

      :telemetry.detach("test-no-conflict")
    end
  end

  describe "cross-node conflict resolution" do
    @describetag :distributed

    setup do
      unless Node.alive?() do
        {:ok, _} = :net_kernel.start([:"quanta_test_#{System.unique_integer([:positive])}", :shortnames])
      end

      :syn.add_node_to_scopes([:actors])
      :syn.set_event_handler(Quanta.Actor.SynEventHandler)

      peer_name = :"peer_#{System.unique_integer([:positive])}"
      code_paths = :code.get_path() |> Enum.flat_map(fn path -> [~c"-pa", path] end)
      {:ok, peer, peer_node} = :peer.start(%{name: peer_name, connection: :standard_io, args: code_paths})

      for app <- [:elixir, :logger, :telemetry, :syn] do
        {:ok, _} = :peer.call(peer, :application, :ensure_all_started, [app])
      end

      :ok = :peer.call(peer, :syn, :add_node_to_scopes, [[:actors]])
      :ok = :peer.call(peer, :syn, :set_event_handler, [Quanta.Actor.SynEventHandler])

      Process.sleep(200)

      on_exit(fn -> try do :peer.stop(peer) catch _, _ -> :ok end end)

      {:ok, peer: peer, peer_node: peer_node}
    end

    test "draining node loses conflict after netsplit heal", ctx do
      actor_id = make_actor_id("netsplit-drain-#{System.unique_integer([:positive])}")

      local_pid = spawn(fn -> receive do :stop -> :ok end end)
      remote_pid = :peer.call(ctx.peer, :erlang, :spawn, [:timer, :sleep, [:infinity]])

      :net_kernel.disconnect(ctx.peer_node)
      Process.sleep(500)

      local_meta = %{node: node(), type: "counter", nonce: 1, activated_at: 0, draining: false}
      :ok = :syn.register(:actors, actor_id, local_pid, local_meta)

      remote_meta = %{node: ctx.peer_node, type: "counter", nonce: 2, activated_at: 0, draining: true}
      :ok = :peer.call(ctx.peer, :syn, :register, [:actors, actor_id, remote_pid, remote_meta])

      true = Node.connect(ctx.peer_node)
      Process.sleep(1_000)

      assert {:ok, ^local_pid} = Quanta.Actor.Registry.lookup(actor_id)
      assert Process.alive?(local_pid)
      refute :peer.call(ctx.peer, Process, :alive?, [remote_pid])

      send(local_pid, :stop)
    end

    test "older registration wins when both non-draining after netsplit heal", ctx do
      actor_id = make_actor_id("netsplit-older-#{System.unique_integer([:positive])}")

      local_pid = spawn(fn -> receive do :stop -> :ok end end)
      remote_pid = :peer.call(ctx.peer, :erlang, :spawn, [:timer, :sleep, [:infinity]])

      :net_kernel.disconnect(ctx.peer_node)
      Process.sleep(500)

      remote_meta = %{node: ctx.peer_node, type: "counter", nonce: 1, activated_at: 0, draining: false}
      :ok = :peer.call(ctx.peer, :syn, :register, [:actors, actor_id, remote_pid, remote_meta])

      Process.sleep(50)

      local_meta = %{node: node(), type: "counter", nonce: 2, activated_at: 0, draining: false}
      :ok = :syn.register(:actors, actor_id, local_pid, local_meta)

      true = Node.connect(ctx.peer_node)
      Process.sleep(1_000)

      assert {:ok, ^remote_pid} = Quanta.Actor.Registry.lookup(actor_id)
      refute Process.alive?(local_pid)
      assert :peer.call(ctx.peer, Process, :alive?, [remote_pid])
    end
  end

  describe "on_process_unregistered/5" do
    test "logs on remote node down" do
      actor_id = make_actor_id("node-down")

      log =
        capture_log(fn ->
          SynEventHandler.on_process_unregistered(
            :actors,
            actor_id,
            self(),
            %{},
            {:syn_remote_scope_node_down, :actors, :remote@host}
          )
        end)

      assert log =~ "node"
      assert log =~ "remote@host"
      assert log =~ "went down"
    end

    test "no-op for normal unregister" do
      actor_id = make_actor_id("normal-unreg")

      log =
        capture_log(fn ->
          SynEventHandler.on_process_unregistered(:actors, actor_id, self(), %{}, :normal)
        end)

      assert log == ""
    end
  end
end
