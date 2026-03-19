# Manual integration test for T28 ephemeral cursor awareness.
#
# Run from the umbrella root:
#   MIX_ENV=test mix run apps/quanta_web/test/manual/ephemeral_manual_test.exs
#
# Exercises ephemeral store/broadcast/cleanup with real processes —
# no Phoenix.ChannelTest, just raw GenServer + subscribe.

alias Quanta.Actor.{CommandRouter, ManifestRegistry, Server}
alias Quanta.{ActorId, Manifest}
alias Quanta.Nifs.EphemeralStore

defmodule EphemeralManualTest do
  @pass IO.ANSI.green() <> "PASS" <> IO.ANSI.reset()
  @fail IO.ANSI.red() <> "FAIL" <> IO.ANSI.reset()

  def run do
    setup()

    results = [
      test("ephemeral set + broadcast to subscriber", &test_set_and_broadcast/0),
      test("sender still receives broadcast (filtering is channel-side)", &test_no_server_side_filter/0),
      test("initial ephemeral state sent on subscribe", &test_initial_state/0),
      test("multi-client: both subscribers receive update", &test_multi_client/0),
      test("unsubscribe cleans up ephemeral key + broadcasts deletion", &test_unsubscribe_cleanup/0),
      test("subscriber death cleans up ephemeral key", &test_subscriber_death_cleanup/0),
      test("non-CRDT actor ignores ephemeral cast", &test_non_crdt_noop/0),
      test("ephemeral data survives across many rapid updates", &test_rapid_updates/0)
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

    :ok =
      ManifestRegistry.put(%Manifest{
        version: "1",
        type: "counter",
        namespace: "manual"
      })

    prev = Application.get_env(:quanta_distributed, :actor_modules, %{})

    Application.put_env(
      :quanta_distributed,
      :actor_modules,
      prev
      |> Map.put({"manual", "crdt_doc"}, Quanta.Web.Test.CrdtDoc)
      |> Map.put({"manual", "counter"}, Quanta.Web.Test.Counter)
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

  defp make_crdt_actor(id) do
    actor_id = %ActorId{namespace: "manual", type: "crdt_doc", id: id}
    {:ok, pid} = CommandRouter.ensure_active(actor_id)
    {actor_id, pid}
  end

  defp make_counter_actor(id) do
    actor_id = %ActorId{namespace: "manual", type: "counter", id: id}
    {:ok, pid} = CommandRouter.ensure_active(actor_id)
    {actor_id, pid}
  end

  # --- Tests ---

  defp test_set_and_broadcast do
    {_, pid} = make_crdt_actor("eph-bcast")
    :ok = Server.subscribe(pid, self(), "alice")
    drain(:ephemeral_state)

    GenServer.cast(pid, {:ephemeral_update, "user:bob", "cursor-at-42", self()})

    receive do
      {:ephemeral_update, encoded, _sender} when is_binary(encoded) -> :ok
    after
      1000 -> raise "did not receive ephemeral_update broadcast"
    end

    Server.force_passivate(pid)
  end

  defp test_no_server_side_filter do
    {_, pid} = make_crdt_actor("eph-nofilter")
    :ok = Server.subscribe(pid, self(), "alice")
    drain(:ephemeral_state)

    # Even though sender_pid == self(), server broadcasts to all (channel filters)
    GenServer.cast(pid, {:ephemeral_update, "user:alice", "my-cursor", self()})

    receive do
      {:ephemeral_update, _encoded, _} -> :ok
    after
      1000 -> raise "server should broadcast to sender too (channel does the filtering)"
    end

    Server.force_passivate(pid)
  end

  defp test_initial_state do
    {_, pid} = make_crdt_actor("eph-init")

    # Set some data before subscribing
    GenServer.cast(pid, {:ephemeral_update, "user:pre", "data", self()})
    Process.sleep(50)

    :ok = Server.subscribe(pid, self(), "viewer")

    receive do
      {:ephemeral_state, bytes} when is_binary(bytes) ->
        # Verify it contains the pre-set data by applying to a fresh store
        {:ok, store} = EphemeralStore.new(30_000)
        :ok = EphemeralStore.apply_encoded(store, bytes)
        {:ok, "data"} = EphemeralStore.get(store, "user:pre")
    after
      1000 -> raise "did not receive initial ephemeral_state"
    end

    Server.force_passivate(pid)
  end

  defp test_multi_client do
    {_, pid} = make_crdt_actor("eph-multi")
    parent = self()

    alice =
      spawn_link(fn ->
        :ok = Server.subscribe(pid, self(), "alice")
        relay_loop(parent)
      end)

    bob =
      spawn_link(fn ->
        :ok = Server.subscribe(pid, self(), "bob")
        relay_loop(parent)
      end)

    Process.sleep(50)

    GenServer.cast(pid, {:ephemeral_update, "user:alice", "cursor-5", :external})

    # Both alice and bob should receive
    receive do
      {:relayed, ^alice, {:ephemeral_update, _, _}} -> :ok
    after
      1000 -> raise "alice did not receive ephemeral broadcast"
    end

    receive do
      {:relayed, ^bob, {:ephemeral_update, _, _}} -> :ok
    after
      1000 -> raise "bob did not receive ephemeral broadcast"
    end

    Process.exit(alice, :normal)
    Process.exit(bob, :normal)
    Server.force_passivate(pid)
  end

  defp test_unsubscribe_cleanup do
    {_, pid} = make_crdt_actor("eph-unsub")

    # Watcher to observe broadcasts
    :ok = Server.subscribe(pid, self(), "watcher")
    drain(:ephemeral_state)

    # Client that will leave
    {:ok, client} = Agent.start_link(fn -> nil end)
    :ok = Server.subscribe(pid, client, "leaving")

    # Set ephemeral data for the leaving user
    GenServer.cast(pid, {:ephemeral_update, "user:leaving", "cursor", self()})

    receive do
      {:ephemeral_update, _, _} -> :ok
    after
      1000 -> raise "did not receive initial ephemeral broadcast"
    end

    # Unsubscribe should trigger cleanup broadcast
    :ok = Server.unsubscribe(pid, client)

    receive do
      {:ephemeral_update, encoded, nil} when is_binary(encoded) -> :ok
    after
      1000 -> raise "did not receive deletion broadcast after unsubscribe"
    end

    Server.force_passivate(pid)
  end

  defp test_subscriber_death_cleanup do
    {_, pid} = make_crdt_actor("eph-death")

    # Watcher
    :ok = Server.subscribe(pid, self(), "watcher")
    drain(:ephemeral_state)

    # Doomed subscriber
    doomed =
      spawn(fn ->
        :ok = Server.subscribe(pid, self(), "doomed")
        receive do: (:exit -> :ok)
      end)

    Process.sleep(50)

    # Set ephemeral data for doomed user
    GenServer.cast(pid, {:ephemeral_update, "user:doomed", "cursor", self()})

    receive do
      {:ephemeral_update, _, _} -> :ok
    after
      1000 -> raise "did not receive ephemeral broadcast"
    end

    # Kill the doomed subscriber — monitor should trigger cleanup
    Process.exit(doomed, :kill)

    receive do
      {:ephemeral_update, encoded, nil} when is_binary(encoded) -> :ok
    after
      2000 -> raise "did not receive deletion broadcast after subscriber death"
    end

    Server.force_passivate(pid)
  end

  defp test_non_crdt_noop do
    {_, pid} = make_counter_actor("eph-noop")

    GenServer.cast(pid, {:ephemeral_update, "user:x", "data", self()})
    Process.sleep(50)

    true = Process.alive?(pid)

    # No messages should arrive
    receive do
      {:ephemeral_update, _, _} -> raise "non-CRDT actor should not broadcast ephemeral"
    after
      0 -> :ok
    end

    Server.force_passivate(pid)
  end

  defp test_rapid_updates do
    {_, pid} = make_crdt_actor("eph-rapid")
    :ok = Server.subscribe(pid, self(), "observer")
    drain(:ephemeral_state)

    # Fire 50 rapid updates
    for i <- 1..50 do
      GenServer.cast(pid, {:ephemeral_update, "user:rapid", "pos-#{i}", :ext})
    end

    # Drain all broadcasts
    Process.sleep(200)
    count = drain_count(:ephemeral_update)
    true = count == 50

    # Verify final value in the store
    actor_state = :sys.get_state(pid)
    {:ok, "pos-50"} = EphemeralStore.get(actor_state.ephemeral_store, "user:rapid")

    Server.force_passivate(pid)
  end

  # --- Helpers ---

  defp drain(tag) do
    receive do
      {^tag, _} -> drain(tag)
    after
      100 -> :ok
    end
  end

  defp drain_count(tag) do
    drain_count(tag, 0)
  end

  defp drain_count(tag, acc) do
    receive do
      {^tag, _, _} -> drain_count(tag, acc + 1)
    after
      0 -> acc
    end
  end

  defp relay_loop(parent) do
    receive do
      msg ->
        send(parent, {:relayed, self(), msg})
        relay_loop(parent)
    end
  end
end

IO.puts("Ephemeral cursor awareness manual test")
IO.puts("=======================================")
EphemeralManualTest.run()
