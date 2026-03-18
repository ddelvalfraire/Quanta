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
    :ets.new(:quanta_actor_init_attempts, [:named_table, :public, :set])
    Quanta.RateLimit.init()

    children = [
      Quanta.HLC.Server,
      Quanta.Wasm.EngineManager,
      Quanta.Wasm.ModuleRegistry,
      Quanta.Actor.ManifestRegistry,
      Quanta.Nats.CoreSupervisor,
      Quanta.Nats.JetStream.Connection,
      Quanta.Actor.DynSup,
      {Task.Supervisor, name: Quanta.SideEffect.TaskSupervisor},
      Quanta.Actor.CommandRouter,
      Quanta.Actor.CompactionScheduler
    ]

    Supervisor.init(children, strategy: :one_for_one)
  end
end
