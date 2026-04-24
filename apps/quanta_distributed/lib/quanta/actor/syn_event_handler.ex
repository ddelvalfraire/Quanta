defmodule Quanta.Actor.SynEventHandler do
  @moduledoc """
  Syn event handler for distributed actor registry conflict resolution and lifecycle events.

  Conflict resolution strategy: if one side is draining, keep the non-draining side.
  Otherwise keep the older registration (by syn timestamp). The loser is killed
  with `Process.exit(pid, :kill)` since syn does not kill it when a custom handler is used.
  """

  require Logger

  alias Quanta.Actor.Registry

  @behaviour :syn_event_handler

  @impl true
  def resolve_registry_conflict(_scope, name, {pid1, meta1, time1}, {pid2, meta2, time2}) do
    {winner, loser} = pick_winner(pid1, meta1, time1, pid2, meta2, time2)

    Logger.info(
      "Registry conflict resolved for #{inspect(name)}: " <>
        "keeping #{inspect(winner)}, killing #{inspect(loser)}"
    )

    Process.exit(loser, :kill)
    winner
  end

  @impl true
  def on_process_registered(:actors, name, pid, meta, :syn_conflict_resolution) do
    mirror_registered(name, pid)

    Quanta.Telemetry.emit(
      [:quanta, :actor, :conflict_resolved],
      %{},
      %{actor_id: name, pid: pid, meta: meta}
    )
  end

  def on_process_registered(:actors, name, pid, _meta, _reason) do
    mirror_registered(name, pid)
    :ok
  end

  def on_process_registered(_scope, _name, _pid, _meta, _reason), do: :ok

  @impl true
  def on_process_unregistered(:actors, name, pid, _meta, {:syn_remote_scope_node_down, :actors, node}) do
    mirror_unregistered(name)

    Logger.warning(
      "Actor #{inspect(name)} (#{inspect(pid)}) unregistered: node #{inspect(node)} went down"
    )
  end

  def on_process_unregistered(:actors, name, _pid, _meta, _reason) do
    mirror_unregistered(name)
    :ok
  end

  def on_process_unregistered(_scope, _name, _pid, _meta, _reason), do: :ok

  defp mirror_registered(%Quanta.ActorId{} = actor_id, pid) when is_pid(pid) do
    Registry.track_local(actor_id, pid)
    :ok
  end

  defp mirror_registered(_name, _pid), do: :ok

  defp mirror_unregistered(%Quanta.ActorId{} = actor_id) do
    Registry.untrack_local(actor_id)
    :ok
  end

  defp mirror_unregistered(_name), do: :ok

  defp pick_winner(pid1, %{draining: true}, _t1, pid2, %{draining: false}, _t2), do: {pid2, pid1}
  defp pick_winner(pid1, %{draining: false}, _t1, pid2, %{draining: true}, _t2), do: {pid1, pid2}

  defp pick_winner(pid1, _meta1, time1, pid2, _meta2, time2) do
    if time1 <= time2, do: {pid1, pid2}, else: {pid2, pid1}
  end
end
