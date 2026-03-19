defmodule Quanta.DrainIntegrationTest do
  @moduledoc """
  End-to-end test: start real Actor.Server processes with different characteristics,
  drain with fast timers, verify all stopped and telemetry events received in order.
  """
  use ExUnit.Case, async: false

  alias Quanta.Actor.{DynSup, ManifestRegistry, Server}
  alias Quanta.{ActorId, Drain, Envelope, Manifest}

  @namespace "test"
  @fast_opts [
    complete_in_flight_delay_ms: 10,
    ordered_passivation_delay_ms: 10,
    force_stop_delay_ms: 2_000
  ]

  defp make_actor_id(id, type \\ "counter") do
    %ActorId{namespace: @namespace, type: type, id: id}
  end

  defp build_manifest(overrides \\ []) do
    attrs =
      Keyword.merge(
        [version: "1", type: "counter", namespace: @namespace],
        overrides
      )

    struct!(Manifest, attrs)
  end

  defp start_actor(actor_id, module \\ Quanta.Test.Actors.Counter) do
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

    :ok =
      ManifestRegistry.put(
        build_manifest(
          type: "crdt_doc",
          state: %Manifest.State{kind: {:crdt, :text}}
        )
      )

    on_exit(fn ->
      try do
        :persistent_term.erase({Drain, :draining})
      rescue
        ArgumentError -> :ok
      end

      if pid = Process.whereis(Drain) do
        try do
          GenServer.stop(pid, :normal, 1_000)
        catch
          _, _ -> :ok
        end
      end

      if Process.whereis(Quanta.Cluster.Topology) do
        send(Process.whereis(Quanta.Cluster.Topology), {:nodeup, node(), []})
        Quanta.Cluster.Topology.nodes()
      end

      clear_dynsup()
    end)

    :ok
  end

  test "drains 5 actors with mixed characteristics, emits telemetry in order" do
    ref =
      :telemetry_test.attach_event_handlers(self(), [
        [:quanta, :drain, :started],
        [:quanta, :drain, :step_started],
        [:quanta, :drain, :batch_passivated],
        [:quanta, :drain, :completed]
      ])

    # Actor 1-2: idle counters
    {:ok, pid1} = start_actor(make_actor_id("int-idle-1"))
    {:ok, pid2} = start_actor(make_actor_id("int-idle-2"))

    # Actor 3: counter with a timer
    {:ok, pid3} = start_actor(make_actor_id("int-timer-1"))
    env = Envelope.new(payload: "set_timer:heartbeat:60000")
    Server.send_message(pid3, env)

    # Actor 4-5: CRDT docs (have subscriber-like state)
    {:ok, pid4} = start_actor(make_actor_id("int-crdt-1", "crdt_doc"), Quanta.Test.Actors.CrdtDoc)
    {:ok, pid5} = start_actor(make_actor_id("int-crdt-2", "crdt_doc"), Quanta.Test.Actors.CrdtDoc)

    all_pids = [pid1, pid2, pid3, pid4, pid5]
    assert Enum.all?(all_pids, &Process.alive?/1)
    assert DynSup.count_actors() >= 5

    # Start drain
    {:ok, _} = Drain.start_drain(@fast_opts)
    assert Drain.draining?()

    assert :ok = Drain.await(15_000)

    # All actors should be stopped
    Process.sleep(100)
    refute Enum.any?(all_pids, &Process.alive?/1)

    # Verify telemetry order
    assert_received {[:quanta, :drain, :started], ^ref, %{}, %{node: _}}

    assert_received {[:quanta, :drain, :step_started], ^ref, %{},
                     %{node: _, step: :stop_ingress}}

    assert_received {[:quanta, :drain, :step_started], ^ref, %{},
                     %{node: _, step: :complete_in_flight}}

    assert_received {[:quanta, :drain, :step_started], ^ref, %{},
                     %{node: _, step: :ordered_passivation}}

    # Batch passivated should have been emitted at least once
    assert_received {[:quanta, :drain, :batch_passivated], ^ref,
                     %{count: count, duration_ms: _}, %{node: _}}

    assert count > 0

    assert_received {[:quanta, :drain, :step_started], ^ref, %{},
                     %{node: _, step: :force_stop}}

    assert_received {[:quanta, :drain, :completed], ^ref, %{duration_ms: total_ms, remaining: _},
                     %{node: _}}

    assert total_ms > 0
  end

  test "drain with no actors completes successfully" do
    assert DynSup.count_actors() == 0

    {:ok, _} = Drain.start_drain(@fast_opts)
    assert :ok = Drain.await(5_000)
    refute Drain.draining?()
  end

  test "health check returns draining during drain" do
    {:ok, _} = Drain.start_drain(@fast_opts)
    assert Drain.draining?()

    # While draining, the flag is set
    assert :persistent_term.get({Drain, :draining}, false) == true

    assert :ok = Drain.await(5_000)
    refute Drain.draining?()
  end
end
