defmodule Quanta.Drain do
  @moduledoc """
  Orchestrates graceful node drain in four timed phases:

  1. **stop_ingress** — unsubscribe from NATS commands, remove self from hash ring,
     mark all local actors as draining
  2. **complete_in_flight** — broadcast drain notification so WebSocket clients reconnect
  3. **ordered_passivation** — passivate actors in priority order (idle first, pending last)
  4. **force_stop** — kill any remaining actors and close NATS connections

  Started on-demand by the drain HTTP endpoint, not supervised at boot.
  Uses `:persistent_term` for the draining flag so the health controller can check
  it at O(1) without message passing.
  """

  use GenServer

  alias Quanta.Actor.{CommandRouter, DynSup, Registry, Server}

  require Logger

  @persistent_term_key {__MODULE__, :draining}
  @batch_size 1000

  @default_total_drain_budget_ms 80_000
  @default_complete_in_flight_delay_ms 2_000
  @default_ordered_passivation_delay_ms 8_000

  defstruct [:caller, :started_at, :step, :remaining_pids, :opts]

  @spec draining?() :: boolean()
  def draining? do
    :persistent_term.get(@persistent_term_key, false)
  end

  @doc """
  ## Options

    * `:complete_in_flight_delay_ms` — delay before broadcasting drain (default 2s)
    * `:ordered_passivation_delay_ms` — delay after broadcast before passivation (default 8s)
    * `:force_stop_delay_ms` — absolute deadline from drain start for force stop (default computed as 80s - other delays)
    * `:broadcast_fn` — zero-arity function called during complete_in_flight to notify
      WebSocket clients (default: no-op)
  """
  @spec start_drain(keyword()) :: {:ok, pid()} | {:error, term()}
  def start_drain(opts \\ []) do
    GenServer.start(__MODULE__, {self(), opts}, name: __MODULE__)
  end

  @spec await(timeout()) :: :ok | :timeout
  def await(timeout \\ 95_000) do
    receive do
      {:drain_complete, __MODULE__} -> :ok
    after
      timeout -> :timeout
    end
  end

  @impl true
  def init({caller, opts}) do
    state = %__MODULE__{
      caller: caller,
      started_at: System.monotonic_time(:millisecond),
      step: :stop_ingress,
      remaining_pids: [],
      opts: opts
    }

    :persistent_term.put(@persistent_term_key, true)

    Quanta.Telemetry.emit([:quanta, :drain, :started], %{}, %{node: node()})

    {:ok, state, {:continue, :stop_ingress}}
  end

  @impl true
  def handle_continue(:stop_ingress, state) do
    Quanta.Telemetry.emit(
      [:quanta, :drain, :step_started],
      %{},
      %{node: node(), step: :stop_ingress}
    )

    safely(:command_router_unsubscribe, fn -> CommandRouter.unsubscribe() end)
    safely(:topology_remove_self, fn -> Quanta.Cluster.Topology.remove_self() end)

    for {actor_id, _pid, _meta} <- Registry.local_actor_ids() do
      safely({:mark_draining, actor_id}, fn -> Registry.mark_draining(actor_id) end)
    end

    delay = Keyword.get(state.opts, :complete_in_flight_delay_ms, @default_complete_in_flight_delay_ms)
    Process.send_after(self(), :complete_in_flight, delay)

    {:noreply, %{state | step: :complete_in_flight}}
  end

  @impl true
  def handle_info(:complete_in_flight, state) do
    Quanta.Telemetry.emit(
      [:quanta, :drain, :step_started],
      %{},
      %{node: node(), step: :complete_in_flight}
    )

    broadcast_fn = Keyword.get(state.opts, :broadcast_fn, fn -> :ok end)
    safely(:broadcast, broadcast_fn)

    delay = Keyword.get(state.opts, :ordered_passivation_delay_ms, @default_ordered_passivation_delay_ms)
    Process.send_after(self(), :ordered_passivation, delay)

    {:noreply, %{state | step: :ordered_passivation}}
  end

  @impl true
  def handle_info(:ordered_passivation, state) do
    Quanta.Telemetry.emit(
      [:quanta, :drain, :step_started],
      %{},
      %{node: node(), step: :ordered_passivation}
    )

    pids = DynSup.list_actor_pids()
    sorted = classify_and_sort(pids)

    state = %{state | step: :passivate_batch, remaining_pids: sorted}
    send(self(), :passivate_batch)

    force_stop_delay = force_stop_remaining_ms(state)
    Process.send_after(self(), :force_stop, force_stop_delay)

    {:noreply, state}
  end

  @impl true
  def handle_info(:passivate_batch, state) do
    {batch, rest} = Enum.split(state.remaining_pids, @batch_size)

    if batch == [] do
      {:noreply, %{state | remaining_pids: []}}
    else
      start_time = System.monotonic_time(:millisecond)

      Enum.each(batch, fn {_priority, pid} ->
        safely({:passivate, pid}, fn -> Server.force_passivate(pid) end)
      end)

      duration_ms = System.monotonic_time(:millisecond) - start_time

      Quanta.Telemetry.emit(
        [:quanta, :drain, :batch_passivated],
        %{count: length(batch), duration_ms: duration_ms},
        %{node: node()}
      )

      if rest != [] do
        send(self(), :passivate_batch)
      end

      {:noreply, %{state | remaining_pids: rest}}
    end
  end

  @impl true
  def handle_info(:force_stop, state) do
    Quanta.Telemetry.emit(
      [:quanta, :drain, :step_started],
      %{},
      %{node: node(), step: :force_stop}
    )

    remaining = DynSup.list_actor_pids()

    Enum.each(remaining, fn pid ->
      safely({:force_stop, pid}, fn -> GenServer.stop(pid, :normal, 5_000) end)
    end)

    Quanta.Nats.Core.close_all()

    duration_ms = System.monotonic_time(:millisecond) - state.started_at

    Quanta.Telemetry.emit(
      [:quanta, :drain, :completed],
      %{duration_ms: duration_ms, remaining: length(remaining)},
      %{node: node()}
    )

    :persistent_term.erase(@persistent_term_key)

    send(state.caller, {:drain_complete, __MODULE__})

    {:stop, :normal, state}
  end

  @impl true
  def handle_info(_msg, state) do
    {:noreply, state}
  end

  @impl true
  def terminate(_reason, _state) do
    :persistent_term.erase(@persistent_term_key)
  rescue
    ArgumentError -> :ok
  end

  defp classify_and_sort(pids) do
    pids
    |> Enum.map(fn pid -> {Server.drain_priority(pid), pid} end)
    |> Enum.sort_by(fn {priority, _pid} -> priority end)
  end

  defp force_stop_remaining_ms(state) do
    elapsed = System.monotonic_time(:millisecond) - state.started_at
    budget = Keyword.get(state.opts, :force_stop_delay_ms, @default_total_drain_budget_ms)
    max(0, budget - elapsed)
  end

  defp safely(label, fun) do
    fun.()
  catch
    kind, reason ->
      Logger.warning("Drain: #{inspect(label)} failed: #{kind} #{inspect(reason)}")
      :error
  end
end
