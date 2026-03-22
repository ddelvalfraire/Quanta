defmodule Quanta.Bridge.Subscriptions do
  @moduledoc """
  Manages NATS subscriptions for the bridge protocol.

  On startup, subscribes to:
  - `d2r_catch_all` — all distributed-to-realtime messages
  - `r2d_wildcard` — all realtime-to-distributed messages (queue group)

  Per-island subscriptions can be added/removed dynamically.
  """

  use GenServer

  alias Quanta.Bridge.Subjects
  alias Quanta.Codec.Bridge, as: BridgeCodec

  require Logger

  @spec start_link(keyword()) :: GenServer.on_start()
  def start_link(opts \\ []) do
    GenServer.start_link(__MODULE__, opts, name: __MODULE__)
  end

  @doc "Subscribe to d2r messages for a specific island."
  @spec subscribe_island(String.t()) :: :ok | {:error, term()}
  def subscribe_island(island_id) do
    GenServer.call(__MODULE__, {:subscribe_island, island_id})
  end

  @doc "Unsubscribe from d2r messages for a specific island."
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
      island_subs: %{}
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

    with {:ok, catch_all} <-
           Quanta.Nats.Core.subscribe(Subjects.d2r_catch_all(ns), nil, self()),
         {:ok, r2d} <-
           Quanta.Nats.Core.subscribe(
             Subjects.r2d_wildcard(ns),
             Subjects.r2d_queue_group(),
             self()
           ) do
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
    if Map.has_key?(state.island_subs, island_id) do
      {:reply, :ok, state}
    else
      subject = Subjects.d2r_wildcard(state.namespace, island_id)

      case Quanta.Nats.Core.subscribe(subject, nil, self()) do
        {:ok, sub} ->
          {:reply, :ok, %{state | island_subs: Map.put(state.island_subs, island_id, sub)}}

        {:error, _} = err ->
          {:reply, err, state}
      end
    end
  end

  @impl true
  def handle_call({:unsubscribe_island, island_id}, _from, state) do
    case Map.pop(state.island_subs, island_id) do
      {nil, _} ->
        {:reply, :ok, state}

      {sub, rest} ->
        Quanta.Nats.Core.unsubscribe(sub)
        {:reply, :ok, %{state | island_subs: rest}}
    end
  end

  @impl true
  def handle_info({:msg, %{topic: topic, body: body}}, state) do
    case BridgeCodec.decode(body) do
      {:ok, header, _payload} ->
        Logger.debug("Bridge msg on #{topic}: #{inspect(header.msg_type)}")

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

  defp namespace do
    Application.get_env(:quanta_distributed, :bridge_namespace, "default")
  end
end
