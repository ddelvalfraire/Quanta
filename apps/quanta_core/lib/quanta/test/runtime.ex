defmodule Quanta.Test.Runtime do
  @moduledoc """
  In-memory, synchronous, deterministic actor runtime for testing.

  No concurrency, no NIFs, no NATS. Effects are interpreted, not executed.
  Virtual time only advances via `advance_time/2`.
  """

  alias Quanta.{ActorId, Effect, Envelope, HLC, ULID}

  defstruct actors: %{},
            mailboxes: %{},
            effects_log: [],
            timers: %{},
            clock: 0,
            modules: %{},
            published: []

  @type t :: %__MODULE__{
          actors: %{ActorId.t() => %{state_data: binary(), module: module()}},
          mailboxes: %{ActorId.t() => [Envelope.t()]},
          effects_log: [{ActorId.t(), Effect.t()}],
          timers: %{{ActorId.t(), String.t()} => %{fire_at: non_neg_integer(), created_by: String.t()}},
          clock: non_neg_integer(),
          modules: %{String.t() => module()},
          published: [{String.t(), binary()}]
        }

  @spec new([{String.t(), module()}]) :: t()
  def new(modules) do
    %__MODULE__{modules: Map.new(modules)}
  end

  @spec spawn_actor(t(), ActorId.t(), binary()) :: {t(), [Effect.t()]}
  def spawn_actor(%__MODULE__{} = rt, %ActorId{} = actor_id, init_payload) do
    module = Map.fetch!(rt.modules, actor_id.type)
    {state_data, effects} = module.init(init_payload)

    rt = put_in(rt.actors[actor_id], %{state_data: state_data, module: module})
    rt = log_effects(rt, actor_id, effects)
    message_id = ULID.generate()
    rt = process_effects(rt, actor_id, effects, message_id)
    {rt, effects}
  end

  @spec send_message(t(), ActorId.t(), binary()) :: {t(), [Effect.t()]}
  def send_message(%__MODULE__{} = rt, %ActorId{} = actor_id, payload) do
    envelope = build_envelope(rt, nil, payload, nil)
    deliver_message(rt, actor_id, envelope)
  end

  @spec send_and_drain(t(), ActorId.t(), binary()) :: {t(), [{ActorId.t(), [Effect.t()]}]}
  def send_and_drain(%__MODULE__{} = rt, %ActorId{} = actor_id, payload) do
    envelope = build_envelope(rt, nil, payload, nil)
    rt = enqueue(rt, actor_id, envelope)
    drain_mailboxes(rt, [])
  end

  @spec advance_time(t(), non_neg_integer()) :: {t(), [{ActorId.t(), [Effect.t()]}]}
  def advance_time(%__MODULE__{} = rt, delta) do
    rt = %{rt | clock: rt.clock + delta}
    {rt, fired_effects} = fire_due_timers(rt)
    {rt, drained} = drain_mailboxes(rt, [])
    {rt, fired_effects ++ drained}
  end

  @spec get_state(t(), ActorId.t()) :: {:ok, binary()} | {:error, :not_found}
  def get_state(%__MODULE__{} = rt, %ActorId{} = actor_id) do
    case Map.get(rt.actors, actor_id) do
      %{state_data: data} -> {:ok, data}
      nil -> {:error, :not_found}
    end
  end

  @spec effects_for(t(), ActorId.t()) :: [Effect.t()]
  def effects_for(%__MODULE__{} = rt, %ActorId{} = actor_id) do
    rt.effects_log
    |> Enum.filter(fn {id, _} -> id == actor_id end)
    |> Enum.map(fn {_, effect} -> effect end)
  end

  @spec published_on(t(), String.t()) :: [binary()]
  def published_on(%__MODULE__{} = rt, channel) do
    rt.published
    |> Enum.filter(fn {ch, _} -> ch == channel end)
    |> Enum.map(fn {_, payload} -> payload end)
  end

  @spec assert_sent(t(), ActorId.t(), ActorId.t(), (binary() -> boolean())) :: :ok | no_return()
  def assert_sent(%__MODULE__{} = rt, from, to, predicate) do
    found =
      rt.effects_log
      |> Enum.any?(fn
        {^from, {:send, ^to, payload}} -> predicate.(payload)
        _ -> false
      end)

    unless found do
      raise ExUnit.AssertionError,
        message: "expected #{inspect(from)} to send a matching message to #{inspect(to)}"
    end

    :ok
  end

  @spec assert_replied([Effect.t()], (binary() -> boolean())) :: :ok | no_return()
  def assert_replied(effects, predicate) do
    found =
      Enum.any?(effects, fn
        {:reply, payload} -> predicate.(payload)
        _ -> false
      end)

    unless found do
      raise ExUnit.AssertionError, message: "expected a matching :reply effect"
    end

    :ok
  end

  # --- Internal ---

  defp deliver_message(rt, actor_id, envelope) do
    case Map.get(rt.actors, actor_id) do
      nil ->
        # Actor not spawned — silently drop (in production this routes to NATS)
        {rt, []}

      actor ->
        {new_state, effects} = actor.module.handle_message(actor.state_data, envelope)

        rt = put_in(rt.actors[actor_id].state_data, new_state)
        rt = log_effects(rt, actor_id, effects)
        message_id = envelope.message_id
        rt = process_effects(rt, actor_id, effects, message_id)
        {rt, effects}
    end
  end

  defp process_effects(rt, actor_id, effects, message_id) do
    Enum.reduce(effects, rt, fn effect, rt ->
      process_effect(rt, actor_id, effect, message_id)
    end)
  end

  defp process_effect(rt, actor_id, {:send, target, payload}, message_id) do
    envelope = build_envelope(rt, actor_id, payload, message_id)
    enqueue(rt, target, envelope)
  end

  defp process_effect(rt, _actor_id, {:publish, channel, payload}, _message_id) do
    %{rt | published: rt.published ++ [{channel, payload}]}
  end

  defp process_effect(rt, actor_id, {:persist, state_bytes}, _message_id) do
    put_in(rt.actors[actor_id].state_data, state_bytes)
  end

  defp process_effect(rt, actor_id, {:set_timer, name, delay_ms}, message_id) do
    entry = %{fire_at: rt.clock + delay_ms, created_by: message_id}
    put_in(rt.timers[{actor_id, name}], entry)
  end

  defp process_effect(rt, actor_id, {:cancel_timer, name}, _message_id) do
    %{rt | timers: Map.delete(rt.timers, {actor_id, name})}
  end

  defp process_effect(rt, _actor_id, {:spawn_actor, new_actor_id, init_payload}, _message_id) do
    if Map.has_key?(rt.actors, new_actor_id) do
      rt
    else
      {rt, _effects} = spawn_actor(rt, new_actor_id, init_payload)
      rt
    end
  end

  defp process_effect(rt, actor_id, :stop_self, _message_id) do
    %{rt | actors: Map.delete(rt.actors, actor_id)}
  end

  # Logged-only effects — no runtime state change
  defp process_effect(rt, _actor_id, {:reply, _payload}, _message_id), do: rt
  defp process_effect(rt, _actor_id, {:emit_telemetry, _, _, _}, _message_id), do: rt
  defp process_effect(rt, _actor_id, {:side_effect, _}, _message_id), do: rt

  defp enqueue(rt, actor_id, envelope) do
    mailbox = Map.get(rt.mailboxes, actor_id, [])
    put_in(rt.mailboxes[actor_id], mailbox ++ [envelope])
  end

  defp drain_mailboxes(rt, acc) do
    case pop_next_message(rt) do
      {nil, rt} ->
        {rt, acc}

      {{actor_id, envelope}, rt} ->
        {rt, effects} = deliver_message(rt, actor_id, envelope)
        drain_mailboxes(rt, acc ++ [{actor_id, effects}])
    end
  end

  defp pop_next_message(rt) do
    case Enum.find(rt.mailboxes, fn {_id, msgs} -> msgs != [] end) do
      nil ->
        {nil, rt}

      {actor_id, [msg | rest]} ->
        rt = put_in(rt.mailboxes[actor_id], rest)
        {{actor_id, msg}, rt}
    end
  end

  defp fire_due_timers(rt) do
    {due, remaining} =
      Enum.split_with(rt.timers, fn {_key, entry} -> entry.fire_at <= rt.clock end)

    rt = %{rt | timers: Map.new(remaining)}

    Enum.reduce(due, {rt, []}, fn {{actor_id, timer_name}, entry}, {rt, acc} ->
      if Map.has_key?(rt.actors, actor_id) do
        actor = Map.fetch!(rt.actors, actor_id)
        {new_state, effects} = actor.module.handle_timer(actor.state_data, timer_name)

        rt = put_in(rt.actors[actor_id].state_data, new_state)
        rt = log_effects(rt, actor_id, effects)
        rt = process_effects(rt, actor_id, effects, entry.created_by)
        {rt, acc ++ [{actor_id, effects}]}
      else
        {rt, acc}
      end
    end)
  end

  defp log_effects(rt, actor_id, effects) do
    entries = Enum.map(effects, fn e -> {actor_id, e} end)
    %{rt | effects_log: rt.effects_log ++ entries}
  end

  defp build_envelope(rt, sender, payload, causation_id) do
    %Envelope{
      message_id: ULID.generate(),
      timestamp: %HLC{wall: rt.clock, logical: 0},
      sender: sender,
      payload: payload,
      causation_id: causation_id,
      metadata: %{}
    }
  end
end
