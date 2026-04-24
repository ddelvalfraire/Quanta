defmodule Quanta.Supervisor do
  @moduledoc """
  Top-level supervisor for the Quanta runtime.

  Composes two sub-supervisors under `:rest_for_one` (HIGH-3):

    1. `Quanta.Infrastructure.Supervisor` — SynConfig, Cluster, HLC, Nats,
       Broadway, registries, side-effect task supervision.
    2. `Quanta.Actor.Supervisor` — DynSup, CommandRouter, Bridge.Subscriptions,
       CompactionScheduler.

  `rest_for_one` means an infrastructure crash (e.g. catastrophic NATS loss)
  restarts the actor runtime, but a crash inside the actor runtime does NOT
  restart infrastructure — preventing actor-layer bugs from recycling
  cluster-critical services.
  """

  use Supervisor

  def start_link(opts \\ []) do
    Supervisor.start_link(__MODULE__, opts, name: __MODULE__)
  end

  @impl true
  def init(_opts) do
    children = [
      Quanta.Infrastructure.Supervisor,
      Quanta.Actor.Supervisor
    ]

    Supervisor.init(children, strategy: :rest_for_one)
  end
end
