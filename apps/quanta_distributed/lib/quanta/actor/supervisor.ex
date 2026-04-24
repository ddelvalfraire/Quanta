defmodule Quanta.Actor.Supervisor do
  @moduledoc """
  Supervises the actor-runtime layer.

  Depends on `Quanta.Infrastructure.Supervisor` (SynConfig, Nats, HLC,
  registries, etc.) via `rest_for_one` composition at the top level
  (`Quanta.Supervisor`). An infrastructure failure restarts this supervisor;
  a failure within this supervisor does NOT restart infrastructure (HIGH-3).

  Members:

    * `Quanta.Actor.DynSup` — partitioned DynamicSupervisor for actor
      processes. `DynSup.Monitor` is already an internal child of DynSup.
    * `Quanta.Actor.CommandRouter` — routes NATS command subjects to actors.
    * `Quanta.Bridge.Subscriptions` — bridge subscriptions over NATS.
    * `Quanta.Actor.CompactionScheduler` — periodic compaction of actor state.
  """

  use Supervisor

  def start_link(opts \\ []) do
    Supervisor.start_link(__MODULE__, opts, name: __MODULE__)
  end

  @impl true
  def init(_opts) do
    children = [
      Quanta.Actor.DynSup,
      Quanta.Actor.CommandRouter,
      Quanta.Bridge.Subscriptions,
      Quanta.Actor.CompactionScheduler
    ]

    Supervisor.init(children, strategy: :one_for_one)
  end
end
