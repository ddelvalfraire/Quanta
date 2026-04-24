defmodule Quanta.Actor.DynSup.Monitor do
  @moduledoc """
  Centralized actor monitor for `Quanta.Actor.DynSup`.

  Owns `Process.monitor/1` refs for every actor pid tracked by the atomic
  counter. When an actor exits, `handle_info({:DOWN, ...}, state)`
  decrements the counter exactly once.

  Prior to this module, `DynSup.track_actor/1` used a bare `spawn/1` to hold
  the monitor. If that spawned process was killed exogenously (e.g. via
  `Process.exit(pid, :kill)`), the `:DOWN` message was lost and the atomic
  counter leaked. Centralizing ownership here means the only way to lose
  a decrement is for this GenServer to die — in which case the supervisor
  restart resets the known-monitored set alongside the atomic.
  """

  use GenServer

  @counter_key :quanta_actor_counter

  @spec child_spec(any()) :: Supervisor.child_spec()
  def child_spec(opts) do
    %{
      id: __MODULE__,
      start: {__MODULE__, :start_link, [opts]},
      type: :worker
    }
  end

  @spec start_link(any()) :: GenServer.on_start()
  def start_link(_opts) do
    GenServer.start_link(__MODULE__, :ok, name: __MODULE__)
  end

  @doc """
  Registers `pid` for monitoring and increments the atomic counter.

  Callers should invoke this exactly once per successful `start_actor/2`.
  """
  @spec track(pid()) :: :ok
  def track(pid) when is_pid(pid) do
    GenServer.cast(__MODULE__, {:track, pid})
  end

  @impl true
  def init(:ok) do
    # Map of monitor ref -> pid. We keep the pid side for debugging; the ref
    # is what the :DOWN message carries.
    {:ok, %{refs: %{}}}
  end

  @impl true
  def handle_cast({:track, pid}, %{refs: refs} = state) do
    ref = Process.monitor(pid)
    :atomics.add(:persistent_term.get(@counter_key), 1, 1)
    {:noreply, %{state | refs: Map.put(refs, ref, pid)}}
  end

  @impl true
  def handle_info({:DOWN, ref, :process, _pid, _reason}, %{refs: refs} = state) do
    case Map.pop(refs, ref) do
      {nil, ^refs} ->
        {:noreply, state}

      {_pid, remaining} ->
        :atomics.sub(:persistent_term.get(@counter_key), 1, 1)
        {:noreply, %{state | refs: remaining}}
    end
  end

  @impl true
  def handle_info(_msg, state) do
    {:noreply, state}
  end
end
