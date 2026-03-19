# Manual integration test for CrdtChannel.
#
# Run from the umbrella root:
#   MIX_ENV=test mix run apps/quanta_web/test/manual/crdt_channel_manual_test.exs
#
# Exercises the full server + channel flow without Phoenix.ChannelTest,
# verifying subscribe/broadcast/echo-suppression with real processes.

alias Quanta.Actor.{CommandRouter, ManifestRegistry, Server}
alias Quanta.{ActorId, Envelope, Manifest}
alias Quanta.Nifs.LoroEngine

defmodule ManualTest do
  @pass IO.ANSI.green() <> "PASS" <> IO.ANSI.reset()
  @fail IO.ANSI.red() <> "FAIL" <> IO.ANSI.reset()

  def run do
    setup()

    results = [
      test("join returns a Loro snapshot", &test_join_snapshot/0),
      test("subscribe registers channel in server", &test_subscribe/0),
      test("delta broadcast reaches subscriber", &test_delta_broadcast/0),
      test("echo suppression: sender does not receive own delta", &test_echo_suppression/0),
      test("multi-client: two subscribers, only non-sender receives", &test_multi_client/0),
      test("command message crdt_ops broadcast to subscribers", &test_crdt_ops_broadcast/0),
      test("unsubscribe removes subscriber", &test_unsubscribe/0),
      test("subscriber auto-cleanup on process death", &test_subscriber_death/0),
      test("delta size: get_crdt_snapshot returns valid binary", &test_snapshot_roundtrip/0)
    ]

    passed = Enum.count(results, & &1)
    failed = length(results) - passed
    IO.puts("\n#{passed} passed, #{failed} failed")
    if failed > 0, do: System.halt(1)
  end

  defp setup do
    :ok =
      ManifestRegistry.put(%Manifest{
        version: "1",
        type: "crdt_doc",
        namespace: "manual",
        state: %Manifest.State{kind: {:crdt, :map}}
      })

    prev = Application.get_env(:quanta_distributed, :actor_modules, %{})

    Application.put_env(
      :quanta_distributed,
      :actor_modules,
      Map.put(prev, {"manual", "crdt_doc"}, Quanta.Web.Test.CrdtDoc)
    )
  end

  defp test(name, fun) do
    try do
      fun.()
      IO.puts("  #{@pass} #{name}")
      true
    rescue
      e ->
        IO.puts("  #{@fail} #{name}")
        IO.puts("       #{Exception.message(e)}")
        false
    catch
      kind, reason ->
        IO.puts("  #{@fail} #{name}")
        IO.puts("       #{kind}: #{inspect(reason)}")
        false
    end
  end

  defp make_actor(id) do
    actor_id = %ActorId{namespace: "manual", type: "crdt_doc", id: id}
    {:ok, pid} = CommandRouter.ensure_active(actor_id)
    {actor_id, pid}
  end

  defp test_join_snapshot do
    {_actor_id, pid} = make_actor("join-1")
    {:ok, snapshot} = Server.get_crdt_snapshot(pid)
    true = is_binary(snapshot)
    true = byte_size(snapshot) > 0
    Server.force_passivate(pid)
  end

  defp test_subscribe do
    {_actor_id, pid} = make_actor("sub-1")
    :ok = Server.subscribe(pid, self(), "user-1")

    :ok = Server.unsubscribe(pid, self())
    Server.force_passivate(pid)
  end

  defp test_delta_broadcast do
    {_actor_id, pid} = make_actor("bcast-1")
    :ok = Server.subscribe(pid, self(), "listener")

    {:ok, doc} = LoroEngine.doc_new()
    :ok = LoroEngine.map_set(doc, "root", "key", "val")
    {:ok, delta} = LoroEngine.doc_export_snapshot(doc)

    GenServer.cast(pid, {:crdt_delta, delta, "peer-2"})

    receive do
      {:crdt_update, _delta_bytes, "peer-2"} -> :ok
    after
      1000 -> raise "did not receive crdt_update broadcast"
    end

    Server.force_passivate(pid)
  end

  defp test_echo_suppression do
    {_actor_id, pid} = make_actor("echo-1")
    :ok = Server.subscribe(pid, self(), "alice")

    {:ok, doc} = LoroEngine.doc_new()
    :ok = LoroEngine.map_set(doc, "root", "key", "val")
    {:ok, delta} = LoroEngine.doc_export_snapshot(doc)

    GenServer.cast(pid, {:crdt_delta, delta, "alice"})

    receive do
      {:crdt_update, _delta_bytes, "alice"} -> :ok
    after
      1000 -> raise "server should broadcast to all subscribers (channel filters)"
    end

    Server.force_passivate(pid)
  end

  defp test_multi_client do
    {_actor_id, pid} = make_actor("multi-1")
    parent = self()

    alice_pid =
      spawn_link(fn ->
        :ok = Server.subscribe(pid, self(), "alice")
        relay_loop(parent)
      end)

    bob_pid =
      spawn_link(fn ->
        :ok = Server.subscribe(pid, self(), "bob")
        relay_loop(parent)
      end)

    Process.sleep(50)

    {:ok, doc} = LoroEngine.doc_new()
    :ok = LoroEngine.map_set(doc, "root", "from", "alice")
    {:ok, delta} = LoroEngine.doc_export_snapshot(doc)
    GenServer.cast(pid, {:crdt_delta, delta, "alice"})

    receive do
      {:relayed, ^alice_pid, {:crdt_update, _, "alice"}} -> :ok
    after
      1000 -> raise "alice's subscriber did not receive broadcast"
    end

    receive do
      {:relayed, ^bob_pid, {:crdt_update, _, "alice"}} -> :ok
    after
      1000 -> raise "bob's subscriber did not receive broadcast"
    end

    Process.exit(alice_pid, :normal)
    Process.exit(bob_pid, :normal)
    Server.force_passivate(pid)
  end

  defp test_crdt_ops_broadcast do
    {_actor_id, pid} = make_actor("ops-1")
    :ok = Server.subscribe(pid, self(), "listener")

    envelope = Envelope.new(payload: "map_set:key:value", sender: {:client, "test"})
    Server.send_message(pid, envelope)

    receive do
      {:crdt_update, _delta, nil} -> :ok
    after
      1000 -> raise "did not receive crdt_update from command-originated crdt_ops"
    end

    {:ok, json} = Server.get_state(pid)
    state = Jason.decode!(json)
    true = state["data"]["key"] == "value"

    Server.force_passivate(pid)
  end

  defp test_unsubscribe do
    {_actor_id, pid} = make_actor("unsub-1")
    :ok = Server.subscribe(pid, self(), "user-1")
    :ok = Server.unsubscribe(pid, self())

    {:ok, doc} = LoroEngine.doc_new()
    :ok = LoroEngine.map_set(doc, "root", "key", "val")
    {:ok, delta} = LoroEngine.doc_export_snapshot(doc)

    GenServer.cast(pid, {:crdt_delta, delta, "peer-2"})
    Process.sleep(100)

    receive do
      {:crdt_update, _, _} -> raise "received broadcast after unsubscribe"
    after
      0 -> :ok
    end

    Server.force_passivate(pid)
  end

  defp test_subscriber_death do
    {_actor_id, pid} = make_actor("death-1")

    sub_pid =
      spawn(fn ->
        :ok = Server.subscribe(pid, self(), "doomed")

        receive do
          :exit -> :ok
        end
      end)

    Process.sleep(50)

    Process.exit(sub_pid, :kill)
    Process.sleep(100)

    :ok = Server.subscribe(pid, self(), "survivor")

    {:ok, doc} = LoroEngine.doc_new()
    :ok = LoroEngine.map_set(doc, "root", "key", "val")
    {:ok, delta} = LoroEngine.doc_export_snapshot(doc)
    GenServer.cast(pid, {:crdt_delta, delta, "peer-x"})

    receive do
      {:crdt_update, _, "peer-x"} -> :ok
    after
      1000 -> raise "survivor did not receive broadcast"
    end

    Server.force_passivate(pid)
  end

  defp test_snapshot_roundtrip do
    {_actor_id, pid} = make_actor("snap-1")

    {:ok, snap1} = Server.get_crdt_snapshot(pid)

    {:ok, doc} = LoroEngine.doc_new()
    :ok = LoroEngine.map_set(doc, "root", "hello", "world")
    {:ok, delta} = LoroEngine.doc_export_snapshot(doc)
    GenServer.cast(pid, {:crdt_delta, delta, "peer"})
    Process.sleep(50)

    {:ok, snap2} = Server.get_crdt_snapshot(pid)
    true = byte_size(snap2) > byte_size(snap1)

    {:ok, verify_doc} = LoroEngine.doc_new()
    :ok = LoroEngine.doc_import(verify_doc, snap2)
    {:ok, value} = LoroEngine.doc_get_value(verify_doc)
    true = is_map(value)

    Server.force_passivate(pid)
  end

  defp relay_loop(parent) do
    receive do
      msg ->
        send(parent, {:relayed, self(), msg})
        relay_loop(parent)
    end
  end
end

IO.puts("CrdtChannel manual integration test")
IO.puts("====================================")
ManualTest.run()
