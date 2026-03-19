defmodule Quanta.Web.Test.Counter do
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
        {new, [{:persist, new}, {:reply, new}]}

      "get" ->
        {state, [{:reply, state}]}

      "no_reply" ->
        {state, []}

      _ ->
        {state, []}
    end
  end

  @impl true
  def handle_timer(state, _), do: {state, []}
end

defmodule Quanta.Web.Test.CrdtDoc do
  @moduledoc false
  @behaviour Quanta.Actor

  @impl true
  def init(_payload) do
    {<<>>, [{:crdt_ops, [{:map_set, "root", "init", true}]}]}
  end

  @impl true
  def handle_message(state, envelope) do
    case envelope.payload do
      "cmd:" <> rest ->
        {state, [{:reply, "ack:" <> rest}]}

      "map_set:" <> rest ->
        [key, value] = String.split(rest, ":", parts: 2)
        {state, [{:crdt_ops, [{:map_set, "data", key, value}]}]}

      _ ->
        {state, []}
    end
  end

  @impl true
  def handle_timer(state, _), do: {state, []}
end
