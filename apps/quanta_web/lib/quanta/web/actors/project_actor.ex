defmodule Quanta.Web.Actors.ProjectActor do
  @moduledoc """
  Actor backing the project-level LoroTree that holds the file hierarchy.

  All state flows through the generic CRDT layer (`crdt_channel` + tree
  manifest), so no custom messages are handled here — this is intentional.
  """

  @behaviour Quanta.Actor

  @impl true
  def init(_payload) do
    {<<>>, []}
  end

  @impl true
  def handle_message(state, _envelope) do
    {state, []}
  end

  @impl true
  def handle_timer(state, _name), do: {state, []}

  @impl true
  def on_passivate(state), do: state
end
