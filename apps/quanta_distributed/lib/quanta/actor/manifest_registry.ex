defmodule Quanta.Actor.ManifestRegistry do
  @moduledoc """
  ETS-backed registry for actor manifests. Keyed by `{namespace, type}`.
  """

  use GenServer

  @table __MODULE__

  def start_link(opts \\ []) do
    GenServer.start_link(__MODULE__, opts, name: __MODULE__)
  end

  @spec get(namespace :: String.t(), type :: String.t()) :: {:ok, Quanta.Manifest.t()} | :error
  def get(namespace, type) do
    case :ets.lookup(@table, {namespace, type}) do
      [{_key, manifest}] -> {:ok, manifest}
      [] -> :error
    end
  end

  @spec put(Quanta.Manifest.t()) :: :ok | {:error, String.t()}
  def put(%Quanta.Manifest{} = manifest) do
    GenServer.call(__MODULE__, {:put, manifest})
  end

  @spec list_types(namespace :: String.t()) :: [Quanta.Manifest.t()]
  def list_types(namespace) do
    :ets.match_object(@table, {{namespace, :_}, :_})
    |> Enum.map(fn {_key, manifest} -> manifest end)
  end

  @impl true
  def init(_opts) do
    table = :ets.new(@table, [:named_table, :set, :protected, read_concurrency: true])
    {:ok, table}
  end

  @impl true
  def handle_call({:put, manifest}, _from, table) do
    key = {manifest.namespace, manifest.type}

    result =
      case :ets.lookup(table, key) do
        [{_key, existing}] ->
          case Quanta.Manifest.validate_update(existing, manifest) do
            :ok ->
              :ets.insert(table, {key, manifest})
              :ok

            {:error, _} = err ->
              err
          end

        [] ->
          :ets.insert(table, {key, manifest})
          :ok
      end

    {:reply, result, table}
  end
end
