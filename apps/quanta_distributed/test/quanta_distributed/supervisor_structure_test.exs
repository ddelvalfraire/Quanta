defmodule QuantaDistributed.SupervisorStructureTest do
  @moduledoc """
  Architectural test for HIGH-3.

  `Quanta.Supervisor` historically listed 15 peers under `:one_for_one`. That
  coupled infrastructure children (Cluster, Nats, HLC) with actor-runtime
  children (DynSup, CommandRouter, Subscriptions) — a crash in the NATS
  connection, for example, left the actor runtime untouched instead of
  restarting dependent children, while an actor-layer crash could restart
  infrastructure it did not own.

  Fix: split into two sub-supervisors and compose with `:rest_for_one` so
  infrastructure restarts cascade into the actor runtime (but not the other
  way around):

    * `Quanta.Infrastructure.Supervisor` — SynConfig, Cluster, HLC, Nats,
      Broadway, TaskSupervisor, etc.
    * `Quanta.Actor.Supervisor` — DynSup, CommandRouter, Bridge.Subscriptions,
      CompactionScheduler.
  """

  use ExUnit.Case, async: false

  describe "top-level supervisor structure (HIGH-3)" do
    test "Quanta.Supervisor uses :rest_for_one strategy" do
      # :sys.get_state returns the raw record for Elixir's Supervisor
      # (tuple shape: {:state, name, strategy, ...}). Strategy is at index 2.
      state = :sys.get_state(Quanta.Supervisor)
      strategy = elem(state, 2)

      assert strategy == :rest_for_one,
             "Quanta.Supervisor must use :rest_for_one so infrastructure " <>
               "failures restart dependent actor children — got " <>
               "#{inspect(strategy)}."
    end

    test "Quanta.Supervisor has 2 or 3 direct children" do
      children = Supervisor.which_children(Quanta.Supervisor)
      count = length(children)

      assert count in 2..3,
             "Quanta.Supervisor should contain the infrastructure + actor " <>
               "sub-supervisors (and optionally one top-level worker) — got " <>
               "#{count} direct children: #{inspect(Enum.map(children, &elem(&1, 0)))}"
    end

    test "Quanta.Supervisor contains Infrastructure and Actor sub-supervisors" do
      ids =
        Quanta.Supervisor
        |> Supervisor.which_children()
        |> Enum.map(fn {id, _pid, _type, _modules} -> id end)

      assert Quanta.Infrastructure.Supervisor in ids,
             "Quanta.Supervisor must list Quanta.Infrastructure.Supervisor as a child — got #{inspect(ids)}"

      assert Quanta.Actor.Supervisor in ids,
             "Quanta.Supervisor must list Quanta.Actor.Supervisor as a child — got #{inspect(ids)}"
    end

    test "Infrastructure.Supervisor is ordered BEFORE Actor.Supervisor" do
      ids =
        Quanta.Supervisor
        |> Supervisor.which_children()
        |> Enum.map(fn {id, _pid, _type, _modules} -> id end)

      infra_idx = Enum.find_index(ids, &(&1 == Quanta.Infrastructure.Supervisor))
      actor_idx = Enum.find_index(ids, &(&1 == Quanta.Actor.Supervisor))

      # In rest_for_one, earlier children restart later children on failure —
      # NOT the other way around. Infrastructure must therefore come first.
      # Note: which_children returns children in REVERSE start order for
      # :one_for_one/:one_for_all/:rest_for_one supervisors, so the earlier
      # child appears LATER in the list.
      assert not is_nil(infra_idx) and not is_nil(actor_idx)
      assert infra_idx > actor_idx,
             "Infrastructure.Supervisor must start BEFORE Actor.Supervisor in " <>
               "the child list (so an Infrastructure crash cascades to Actor)."
    end
  end

  describe "Infrastructure.Supervisor composition (HIGH-3)" do
    test "contains the expected infrastructure roles" do
      ids =
        Quanta.Infrastructure.Supervisor
        |> Supervisor.which_children()
        |> Enum.map(fn {id, _pid, _type, _modules} -> id end)
        |> MapSet.new()

      expected = [
        Quanta.SynConfig,
        Quanta.HLC.Server,
        Quanta.Nats.CoreSupervisor
      ]

      for role <- expected do
        assert MapSet.member?(ids, role),
               "Quanta.Infrastructure.Supervisor missing #{inspect(role)}. " <>
                 "Got: #{inspect(MapSet.to_list(ids))}"
      end
    end
  end

  describe "Actor.Supervisor composition (HIGH-3)" do
    test "contains the expected actor-runtime roles" do
      ids =
        Quanta.Actor.Supervisor
        |> Supervisor.which_children()
        |> Enum.map(fn {id, _pid, _type, _modules} -> id end)
        |> MapSet.new()

      expected = [
        Quanta.Actor.DynSup,
        Quanta.Actor.CommandRouter
      ]

      for role <- expected do
        assert MapSet.member?(ids, role),
               "Quanta.Actor.Supervisor missing #{inspect(role)}. " <>
                 "Got: #{inspect(MapSet.to_list(ids))}"
      end
    end
  end
end
