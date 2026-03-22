defmodule Quanta.Bridge.Subscriptions do
  @moduledoc """
  Manages NATS subscriptions for the bridge protocol.

  On startup, subscribes to:
  - `d2r_catch_all` with queue group — all distributed-to-realtime messages
  - `r2d_wildcard` with queue group — all realtime-to-distributed messages

  Per-island routing is tracked in GenServer state (not as separate NATS
  subscriptions) to avoid duplicate delivery against the catch-all.
  """

  use GenServer

  alias Quanta.Bridge.Subjects
  alias Quanta.Codec.Bridge, as: BridgeCodec

  require Logger

  @spec start_link(keyword()) :: GenServer.on_start()
  def start_link(opts \\ []) do
    GenServer.start_link(__MODULE__, opts, name: __MODULE__)
  end

  @doc "Register an island for d2r message routing."
  @spec subscribe_island(String.t()) :: :ok | {:error, term()}
  def subscribe_island(island_id) do
    GenServer.call(__MODULE__, {:subscribe_island, island_id})
  end

  @doc "Unregister an island from d2r message routing."
  @spec unsubscribe_island(String.t()) :: :ok
  def unsubscribe_island(island_id) do
    GenServer.call(__MODULE__, {:unsubscribe_island, island_id})
  end

  @impl true
  def init(_opts) do
    state = %{
      namespace: namespace(),
      catch_all_sub: nil,
      r2d_sub: nil,
      active_islands: MapSet.new()
    }

    if Process.whereis(Quanta.Nats.Core.connection(0)) do
      {:ok, state, {:continue, :subscribe}}
    else
      Logger.info("Bridge.Subscriptions started without NATS (no connection)")
      {:ok, state}
    end
  end

  @impl true
  def handle_continue(:subscribe, state) do
    ns = state.namespace

    with {:ok, catch_all_subject} <- Subjects.d2r_catch_all(ns),
         {:ok, catch_all} <-
           Quanta.Nats.Core.subscribe(catch_all_subject, Subjects.d2r_queue_group(), self()),
         {:ok, r2d_subject} <- Subjects.r2d_wildcard(ns),
         {:ok, r2d} <-
           Quanta.Nats.Core.subscribe(r2d_subject, Subjects.r2d_queue_group(), self()) do
      Logger.info("Bridge.Subscriptions connected (ns=#{ns})")
      {:noreply, %{state | catch_all_sub: catch_all, r2d_sub: r2d}}
    else
      {:error, reason} ->
        Logger.error("Bridge.Subscriptions failed to subscribe: #{inspect(reason)}")
        Process.send_after(self(), :retry_subscribe, 5_000)
        {:noreply, state}
    end
  catch
    :exit, reason ->
      Logger.warning("Bridge.Subscriptions NATS not ready, retrying: #{inspect(reason)}")
      Process.send_after(self(), :retry_subscribe, 5_000)
      {:noreply, state}
  end

  @impl true
  def handle_call({:subscribe_island, island_id}, _from, state) do
    {:reply, :ok, %{state | active_islands: MapSet.put(state.active_islands, island_id)}}
  end

  @impl true
  def handle_call({:unsubscribe_island, island_id}, _from, state) do
    {:reply, :ok, %{state | active_islands: MapSet.delete(state.active_islands, island_id)}}
  end

  @impl true
  def handle_info({:msg, %{topic: topic, body: body, reply_to: reply_to}}, state) do
    case BridgeCodec.decode(body) do
      {:ok, header, payload} ->
        dispatch(topic, header, payload, reply_to, state)

      {:error, reason} ->
        Logger.warning("Bridge.Subscriptions: decode error on #{topic}: #{reason}")
    end

    {:noreply, state}
  end

  @impl true
  def handle_info(:retry_subscribe, state) do
    {:noreply, state, {:continue, :subscribe}}
  end

  @impl true
  def handle_info(_msg, state) do
    {:noreply, state}
  end

  defp dispatch(topic, header, _payload, _reply_to, _state) do
    Logger.debug("Bridge msg on #{topic}: #{inspect(header.msg_type)}")
  end

  defp namespace do
    Application.get_env(:quanta_distributed, :bridge_namespace, "default")
  end
end
