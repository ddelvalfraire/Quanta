defmodule Quanta.Broadway.PipelineSupervisor do
  @moduledoc """
  DynamicSupervisor for Broadway event-processing pipelines.

  Each pipeline processes events from a specific JetStream stream
  filtered by namespace and type.
  """

  use DynamicSupervisor

  alias Quanta.Broadway.EventProcessor

  def start_link(opts \\ []) do
    DynamicSupervisor.start_link(__MODULE__, opts, name: __MODULE__)
  end

  @impl true
  def init(_opts) do
    DynamicSupervisor.init(strategy: :one_for_one)
  end

  @doc """
  Starts a Broadway pipeline for the given namespace and type.

  See `Quanta.Broadway.EventProcessor` for supported options.
  """
  @spec start_pipeline(String.t(), String.t(), keyword()) ::
          DynamicSupervisor.on_start_child()
  def start_pipeline(namespace, type, opts \\ []) do
    pipeline_opts =
      opts
      |> Keyword.merge(namespace: namespace, type: type)

    DynamicSupervisor.start_child(__MODULE__, {EventProcessor, pipeline_opts})
  end

  @spec stop_pipeline(String.t(), String.t()) :: :ok | {:error, :not_found}
  def stop_pipeline(namespace, type) do
    name = EventProcessor.pipeline_name(namespace, type)

    case GenServer.whereis(name) do
      nil ->
        {:error, :not_found}

      pid ->
        DynamicSupervisor.terminate_child(__MODULE__, pid)
    end
  end

  @spec list_pipelines() :: [{:undefined, pid() | :restarting, :worker | :supervisor, [module()]}]
  def list_pipelines do
    DynamicSupervisor.which_children(__MODULE__)
  end
end
