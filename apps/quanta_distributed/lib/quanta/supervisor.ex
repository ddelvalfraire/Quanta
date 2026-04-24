defmodule Quanta.Supervisor do
  @moduledoc """
  Top-level supervisor for the Quanta runtime.

  Manages the HLC clock, WASM engine, actor infrastructure,
  and side-effect task supervision.
  """

  use Supervisor

  def start_link(opts \\ []) do
    Supervisor.start_link(__MODULE__, opts, name: __MODULE__)
  end

  @impl true
  def init(_opts) do
    Quanta.RateLimit.init()

    topologies = Application.get_env(:libcluster, :topologies, [])

    children = [
      # MUST be first — configures :syn scopes + event handler before any
      # child that depends on syn (e.g. Quanta.Actor.DynSup, CommandRouter).
      Quanta.SynConfig,
      {Cluster.Supervisor, [topologies, [name: Quanta.ClusterSupervisor]]},
      Quanta.Cluster.Topology,
      %{id: Quanta.Actor.CrdtPubSub, start: {:pg, :start_link, [Quanta.Actor.CrdtPubSub]}},
      Quanta.HLC.Server,
      Quanta.Wasm.EngineManager,
      Quanta.Wasm.ModuleRegistry,
      Quanta.Actor.ManifestRegistry,
      Quanta.Actor.SchemaEvolution,
      Quanta.Nats.CoreSupervisor,
      Quanta.Nats.JetStream.Connection,
      Quanta.Broadway.PipelineSupervisor,
      Quanta.Actor.DynSup,
      {Task.Supervisor, name: Quanta.SideEffect.TaskSupervisor},
      Quanta.Actor.CommandRouter,
      Quanta.Bridge.Subscriptions,
      Quanta.Actor.CompactionScheduler
    ]

    Supervisor.init(children, strategy: :one_for_one)
  end
end
