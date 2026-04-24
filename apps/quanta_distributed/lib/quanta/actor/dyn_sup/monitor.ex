defmodule Quanta.Actor.DynSup.Monitor do
  @moduledoc """
  Centralized actor monitor for `Quanta.Actor.DynSup`.

  Owns `Process.monitor/1` refs for every actor pid tracked by the atomic
  counter. When an actor exits, `handle_info({:DOWN, ...}, state)`
  decrements the counter exactly once.

  Prior to this module, `DynSup.track_actor/1` used a bare `spawn/1` to hold
  the monitor. If that spawned process was killed exogenously (e.g. via
  `Process.exit(pid, :kill)`), the `:DOWN` message was lost and the atomic
  counter leaked.

  ## Restart invariant (CRITICAL-1)

  If this GenServer crashes, its parent `:one_for_one` supervisor restarts
  only the Monitor — `DynSup.start_link/1` is NOT re-entered, so the atomic
  counter retains whatever value it had before the crash. To avoid the
  counter permanently inflating (all previously-tracked pids are now
  un-monitored and their future exits would no-op here), `init/1` rebuilds
  monitor refs by scanning the live `Quanta.Actor.DynSup` partition tree
  and resets the atomic counter to the observed live-actor count.

  Race note: between the scan and the `Process.monitor/1` calls, an actor
  may exit. That is safe — `Process.monitor/1` on a dead pid fires `:DOWN`
  immediately, which `handle_info/2` delivers and decrements correctly.
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
    # Rebuild monitor state by scanning the live DynSup partition tree.
    # Covers both the cold-start case (empty tree → empty refs, counter 0)
    # and the post-crash restart case (non-empty tree → re-monitor all live
    # pids and reset the atomic to the observed count).
    pids = safe_list_actor_pids()

    refs =
      Enum.reduce(pids, %{}, fn pid, acc ->
        ref = Process.monitor(pid)
        Map.put(acc, ref, pid)
      end)

    :atomics.put(:persistent_term.get(@counter_key), 1, map_size(refs))

    {:ok, %{refs: refs}}
  end

  # Enumerates live actor pids across every partition of the DynSup
  # PartitionSupervisor. On cold boot the PartitionSupervisor is declared
  # before Monitor in `DynSup.start_link/1`'s child list, so it is already
  # registered by the time this runs; on crash-restart it is still up
  # (its sibling was the one that died). We guard defensively just in case
  # so a transient lookup failure never prevents Monitor from starting.
  defp safe_list_actor_pids do
    case Process.whereis(Quanta.Actor.DynSup) do
      nil -> []
      _pid -> Quanta.Actor.DynSup.list_actor_pids()
    end
  rescue
    _ -> []
  catch
    :exit, _ -> []
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
