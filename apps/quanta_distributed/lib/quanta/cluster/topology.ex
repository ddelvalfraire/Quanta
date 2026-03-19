defmodule Quanta.Cluster.Topology do
  @moduledoc false

  use GenServer

  require Logger

  @persistent_term_key {__MODULE__, :ring}

  @spec start_link(keyword()) :: GenServer.on_start()
  def start_link(opts \\ []) do
    GenServer.start_link(__MODULE__, opts, name: __MODULE__)
  end

  @spec nodes() :: [node()]
  def nodes do
    GenServer.call(__MODULE__, :nodes)
  end

  @spec healthy?() :: boolean()
  def healthy? do
    GenServer.call(__MODULE__, :healthy?)
  end

  @spec ring() :: {:ok, pid()} | {:error, :not_ready}
  def ring do
    case :persistent_term.get(@persistent_term_key, nil) do
      nil -> {:error, :not_ready}
      pid -> {:ok, pid}
    end
  end

  @impl true
  def init(opts) do
    :net_kernel.monitor_nodes(true, node_type: :visible)

    min_nodes = Keyword.get(opts, :min_nodes, 1)
    ring_opts = Keyword.get(opts, :ring, [])
    initial_nodes = [node() | Node.list()]

    {:ok, ring} =
      ExHashRing.Ring.start_link(Keyword.merge([nodes: initial_nodes], ring_opts))

    :persistent_term.put(@persistent_term_key, ring)

    {:ok, %{ring: ring, min_nodes: min_nodes, nodes: MapSet.new(initial_nodes)}}
  end

  @impl true
  def handle_call(:nodes, _from, state) do
    {:reply, MapSet.to_list(state.nodes), state}
  end

  @impl true
  def handle_call(:healthy?, _from, state) do
    {:reply, MapSet.size(state.nodes) >= state.min_nodes, state}
  end

  @impl true
  def handle_info({:nodeup, node, _info}, state) do
    if MapSet.member?(state.nodes, node) do
      {:noreply, state}
    else
      Logger.info("Node joined cluster: #{node}")
      {:ok, _} = ExHashRing.Ring.add_node(state.ring, node)

      new_nodes = MapSet.put(state.nodes, node)

      Quanta.Telemetry.emit(
        [:quanta, :cluster, :node_up],
        %{count: MapSet.size(new_nodes)},
        %{node: node}
      )

      {:noreply, %{state | nodes: new_nodes}}
    end
  end

  @impl true
  def handle_info({:nodedown, node, _info}, state) do
    if MapSet.member?(state.nodes, node) do
      Logger.info("Node left cluster: #{node}")
      {:ok, _} = ExHashRing.Ring.remove_node(state.ring, node)

      new_nodes = MapSet.delete(state.nodes, node)

      Quanta.Telemetry.emit(
        [:quanta, :cluster, :node_down],
        %{count: MapSet.size(new_nodes)},
        %{node: node}
      )

      {:noreply, %{state | nodes: new_nodes}}
    else
      {:noreply, state}
    end
  end

  @impl true
  def handle_info(_msg, state), do: {:noreply, state}

  @impl true
  def terminate(_reason, _state) do
    :persistent_term.erase(@persistent_term_key)
  rescue
    ArgumentError -> :ok
  end
end
