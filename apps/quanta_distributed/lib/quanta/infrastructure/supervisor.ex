defmodule Quanta.Infrastructure.Supervisor do
  @moduledoc """
  Supervises the infrastructure layer that the actor runtime depends on.

  Children are `one_for_one` within this supervisor: each infra component
  restarts independently. The parent `Quanta.Supervisor` uses `rest_for_one`
  to compose this supervisor with `Quanta.Actor.Supervisor`, so a catastrophic
  infrastructure failure that exceeds this sub-supervisor's restart budget
  cascades into restarting the actor runtime — but not the other way around
  (HIGH-3).

  Members:

    * `Quanta.SynConfig` — must come first, configures syn scopes + handler.
    * `Cluster.Supervisor` / `Quanta.Cluster.Topology` — cluster membership.
    * `Quanta.Actor.CrdtPubSub` — :pg group for CRDT fan-out.
    * `Quanta.HLC.Server` — Hybrid Logical Clock.
    * `Quanta.Wasm.EngineManager` / `Quanta.Wasm.ModuleRegistry` — WASM engine.
    * `Quanta.Actor.ManifestRegistry` / `Quanta.Actor.SchemaEvolution` —
      manifest and schema state.
    * `Quanta.Nats.CoreSupervisor` / `Quanta.Nats.JetStream.Connection` —
      NATS connectivity.
    * `Quanta.Broadway.PipelineSupervisor` — ingress pipeline.
    * `Task.Supervisor` at `Quanta.SideEffect.TaskSupervisor` — side-effect
      task supervision (used by actors, but logically infra).
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
      # child that depends on syn.
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
      {Task.Supervisor, name: Quanta.SideEffect.TaskSupervisor}
    ]

    Supervisor.init(children, strategy: :one_for_one)
  end
end
