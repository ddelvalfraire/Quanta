defmodule Quanta.Test.Actors.Counter do
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

      "set_timer:" <> rest ->
        [name, ms] = String.split(rest, ":", parts: 2)
        {state, [{:set_timer, name, String.to_integer(ms)}]}

      "cancel_timer:" <> name ->
        {state, [{:cancel_timer, name}]}

      "send:" <> target_id ->
        target = %Quanta.ActorId{namespace: "test", type: "counter", id: target_id}
        {state, [{:send, target, "inc"}]}

      "spawn:" <> target_id ->
        target = %Quanta.ActorId{namespace: "test", type: "counter", id: target_id}
        {state, [{:spawn_actor, target, <<>>}]}

      "stop" ->
        {state, [:stop_self]}

      "side_effect" ->
        {state, [{:side_effect, {Kernel, :send, [self(), :side_effect_ran]}}]}

      "telemetry" ->
        {state, [{:emit_telemetry, "test_event", %{value: 1}, %{actor: "counter"}}]}

      "publish:" <> channel ->
        {state, [{:publish, channel, "published_payload"}]}

      "respond:" <> reply_payload ->
        case envelope.sender do
          %Quanta.ActorId{} = sender -> {state, [{:send, sender, reply_payload}]}
          _ -> {state, []}
        end

      _ ->
        {state, []}
    end
  end

  @impl true
  def handle_timer(state, "tick") do
    <<count::64>> = state
    new = <<count + 10::64>>
    {new, [{:persist, new}]}
  end

  @impl true
  def handle_timer(state, _), do: {state, []}

  @impl true
  def on_passivate(state), do: state
end

defmodule Quanta.Test.Actors.Echo do
  @moduledoc false
  @behaviour Quanta.Actor

  @impl true
  def init(payload), do: {payload, [{:reply, "initialized"}]}

  @impl true
  def handle_message(state, envelope) do
    {state, [{:reply, "echo:" <> envelope.payload}]}
  end

  @impl true
  def handle_timer(state, _), do: {state, []}
end

defmodule Quanta.Test.Actors.Responder do
  @moduledoc false
  @behaviour Quanta.Actor

  @impl true
  def init(_payload), do: {"idle", []}

  @impl true
  def handle_message(_state, envelope) do
    case envelope.payload do
      "ask:" <> target_id ->
        target = %Quanta.ActorId{namespace: "test", type: "counter", id: target_id}
        {"waiting", [{:send, target, "respond:pong"}]}

      _ ->
        {"got:" <> envelope.payload, [{:reply, envelope.payload}]}
    end
  end

  @impl true
  def handle_timer(state, _), do: {state, []}
end

defmodule Quanta.Test.Actors.Failer do
  @moduledoc false
  @behaviour Quanta.Actor

  @impl true
  def init(_payload), do: raise("init failure")

  @impl true
  def handle_message(state, _envelope), do: {state, []}

  @impl true
  def handle_timer(state, _), do: {state, []}
end
