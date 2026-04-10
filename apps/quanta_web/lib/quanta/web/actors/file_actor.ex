defmodule Quanta.Web.Actors.FileActor do
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
