defmodule Quanta.Bench.BenchCounter do
  @moduledoc false
  @behaviour Quanta.Actor

  @impl true
  def init(_payload), do: {<<0::64>>, []}

  @impl true
  def handle_message(state, envelope) do
    <<count::64>> = state

    case envelope.payload do
      "inc" ->
        new = <<count + 1::64>>
        {new, [{:reply, new}]}

      "get" ->
        {state, [{:reply, state}]}

      _ ->
        {state, []}
    end
  end

  @impl true
  def handle_timer(state, _name), do: {state, []}
end
