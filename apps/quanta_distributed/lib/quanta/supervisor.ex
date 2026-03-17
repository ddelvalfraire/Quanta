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
    children = [
      Quanta.HLC.Server,
      Quanta.Wasm.EngineManager,
      Quanta.Wasm.ModuleRegistry,
      Quanta.Actor.ManifestRegistry,
      Quanta.Actor.DynSup,
      {Task.Supervisor, name: Quanta.SideEffect.TaskSupervisor},
      Quanta.Actor.CompactionScheduler
    ]

    Supervisor.init(children, strategy: :one_for_one)
  end
end
