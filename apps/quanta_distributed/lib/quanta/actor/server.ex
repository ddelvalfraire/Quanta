defmodule Quanta.Actor.Server do
  @moduledoc """
  Actor lifecycle GenServer — activation, message dispatch, passivation.

  In Phase 1 (no WASM), dispatches to an Elixir module implementing
  `Quanta.Actor`. The module is passed via `module:` in start opts.
  """

  use GenServer

  alias Quanta.Actor.{EffectExecutor, ManifestRegistry, Registry}
  alias Quanta.Envelope

  require Logger

  @mailbox_warn_threshold 1_000
  @mailbox_shed_threshold 5_000
  @mailbox_critical_threshold 10_000
  @init_attempts_table :quanta_actor_init_attempts

  defstruct [
    :actor_id,
    :module,
    :manifest,
    :state_data,
    :idle_timer_ref,
    :activated_at,
    status: :activating,
    events_since_snapshot: 0,
    named_timers: %{},
    pending_replies: %{},
    message_count: 0
  ]

  @type t :: %__MODULE__{
          actor_id: Quanta.ActorId.t() | nil,
          module: module() | nil,
          manifest: Quanta.Manifest.t() | nil,
          state_data: binary() | nil,
          idle_timer_ref: reference() | nil,
          activated_at: integer() | nil,
          status: :activating | :active,
          events_since_snapshot: non_neg_integer(),
          named_timers: %{String.t() => map()},
          pending_replies: %{String.t() => {GenServer.from(), reference()}},
          message_count: non_neg_integer()
        }

  def child_spec(opts) do
    actor_id = Keyword.fetch!(opts, :actor_id)

    %{
      id: actor_id,
      start: {__MODULE__, :start_link, [opts]},
      restart: :transient
    }
  end

  def start_link(opts) do
    GenServer.start_link(__MODULE__, opts)
  end

  @spec send_message(pid(), Envelope.t(), timeout()) ::
          {:ok, binary()} | {:ok, :no_reply} | {:error, term()}
  def send_message(pid, %Envelope{} = envelope, timeout \\ 30_000) do
    GenServer.call(pid, {:message, envelope}, timeout)
  catch
    :exit, {:timeout, _} -> {:error, :actor_timeout}
    :exit, reason -> {:error, reason}
  end

  @spec get_state(pid()) :: {:ok, binary()} | {:error, term()}
  def get_state(pid) do
    GenServer.call(pid, :get_state)
  end

  @spec get_meta(pid()) :: {:ok, map()}
  def get_meta(pid) do
    GenServer.call(pid, :get_meta)
  end

  @spec force_passivate(pid()) :: :ok
  def force_passivate(pid) do
    GenServer.call(pid, :force_passivate)
  end

  @impl true
  def init(opts) do
    actor_id = Keyword.fetch!(opts, :actor_id)
    module = Keyword.fetch!(opts, :module)

    Process.flag(:message_queue_data, :off_heap)

    case Registry.register(actor_id) do
      :ok ->
        Logger.metadata(
          actor_namespace: actor_id.namespace,
          actor_type: actor_id.type,
          actor_id: actor_id.id
        )

        {:ok, %__MODULE__{actor_id: actor_id, module: module}, {:continue, :load_state}}

      {:error, :already_registered} ->
        {:stop, {:already_registered, actor_id}}
    end
  end

  @impl true
  def handle_continue(:load_state, state) do
    case ManifestRegistry.get(state.actor_id.namespace, state.actor_id.type) do
      {:ok, manifest} ->
        activate(state, manifest)

      :error ->
        Logger.error("No manifest for #{inspect(state.actor_id)}")
        {:stop, :no_manifest, state}
    end
  end

  @impl true
  def handle_call({:message, envelope}, from, state) do
    case check_mailbox(state) do
      level when level in [:shedding, :critical] ->
        {:reply, {:error, :overloaded}, state}

      _ ->
        {reply, state, stop?, sent_ids} = dispatch_message(state, envelope)

        cond do
          stop? && reply -> {:stop, :normal, reply, state}
          stop? -> {:stop, :normal, {:ok, :no_reply}, state}
          reply -> {:reply, reply, state}
          reply == nil and sent_ids != [] ->
            state = stash_pending_replies(from, sent_ids, state)
            {:noreply, state}
          true -> {:reply, {:ok, :no_reply}, state}
        end
    end
  end

  @impl true
  def handle_call(:get_state, _from, state) do
    {:reply, {:ok, state.state_data}, state}
  end

  @impl true
  def handle_call(:get_meta, _from, state) do
    meta = %{
      actor_id: state.actor_id,
      status: state.status,
      message_count: state.message_count,
      activated_at: state.activated_at
    }

    {:reply, {:ok, meta}, state}
  end

  @impl true
  def handle_call(:force_passivate, _from, state) do
    :telemetry.execute(
      [:quanta, :actor, :passivate],
      %{},
      %{actor_id: state.actor_id, reason: :force}
    )

    call_on_passivate(state)
    Registry.deregister(state.actor_id)
    {:stop, :normal, :ok, state}
  end

  @impl true
  def handle_cast({:incoming_message, envelope}, state) do
    case check_mailbox(state) do
      level when level in [:shedding, :critical] ->
        {:noreply, state}

      _ ->
        {pending_from, state} = pop_pending_reply(envelope, state)
        {reply, state, stop?, _sent_ids} = dispatch_message(state, envelope)

        state =
          if pending_from && reply do
            GenServer.reply(pending_from, reply)
            cancel_pending_replies_for(pending_from, state)
          else
            state
          end

        if stop?, do: {:stop, :normal, state}, else: {:noreply, state}
    end
  end

  @impl true
  def handle_info(:passivate, state) do
    :telemetry.execute(
      [:quanta, :actor, :passivate],
      %{},
      %{actor_id: state.actor_id, reason: :idle}
    )

    call_on_passivate(state)
    Registry.deregister(state.actor_id)
    {:stop, :normal, state}
  end

  @impl true
  def handle_info({:timer_fire, name}, state) do
    case Map.pop(state.named_timers, name) do
      {nil, _} ->
        {:noreply, state}

      {timer_entry, named_timers} ->
        state = %{state | named_timers: named_timers}
        state = reset_idle_timer(state)

        envelope =
          Envelope.new(
            timestamp: Quanta.HLC.Server.now(),
            causation_id: timer_entry.created_by,
            sender: :system,
            payload: <<>>,
            metadata: %{"timer_name" => name}
          )

        {new_state, effects} = state.module.handle_timer(state.state_data, name)
        state = %{state | state_data: new_state}
        {_reply, state, stop?, _sent_ids} = process_effects(effects, state, envelope)
        if stop?, do: {:stop, :normal, state}, else: {:noreply, state}
    end
  end

  @impl true
  def handle_info({:pending_reply_timeout, msg_id}, state) do
    case Map.pop(state.pending_replies, msg_id) do
      {nil, _} ->
        {:noreply, state}

      {{from, _timer_ref}, pending_replies} ->
        GenServer.reply(from, {:error, :actor_timeout})
        {:noreply, %{state | pending_replies: pending_replies}}
    end
  end

  @impl true
  def handle_info({:subscriber_left, _user_id}, state) do
    {:noreply, reset_idle_timer(state)}
  end

  @impl true
  def handle_info(_msg, state) do
    {:noreply, state}
  end

  defp activate(state, manifest) do
    state = %{state | manifest: manifest}
    meta = %{actor_id: state.actor_id}

    try do
      state =
        :telemetry.span([:quanta, :actor, :activate], meta, fn ->
          {state_data, init_effects} = state.module.init(<<>>)

          state = %{
            state
            | state_data: state_data,
              status: :active,
              activated_at: System.monotonic_time()
          }

          state = process_init_effects(init_effects, state)
          state = schedule_idle_timer(state)
          clear_init_failures(state.actor_id)

          {state, meta}
        end)

      {:noreply, state}
    rescue
      e ->
        stacktrace = __STACKTRACE__
        handle_init_failure(state, e, stacktrace)
    end
  end

  defp dispatch_message(state, envelope) do
    Logger.metadata(message_id: envelope.message_id)
    state = reset_idle_timer(state)
    meta = %{actor_id: state.actor_id, message_id: envelope.message_id}

    :telemetry.span([:quanta, :actor, :message], meta, fn ->
      {new_state, effects} = state.module.handle_message(state.state_data, envelope)
      state = %{state | state_data: new_state, message_count: state.message_count + 1}
      result = process_effects(effects, state, envelope)
      {result, meta}
    end)
  end

  defp process_init_effects(effects, state) do
    Enum.reduce(effects, state, fn
      {:persist, data}, state ->
        if byte_size(data) > state.manifest.state.max_size_bytes do
          raise "Persist failed during init for #{inspect(state.actor_id)}: state_too_large"
        end

        %{state | state_data: data, events_since_snapshot: state.events_since_snapshot + 1}

      {:set_timer, name, delay_ms}, state
      when is_integer(delay_ms) and delay_ms > 0 ->
        max = state.manifest.resources.max_timers

        if map_size(state.named_timers) >= max do
          Logger.warning("Max timers (#{max}) reached during init, dropping timer #{name}")
          state
        else
          ref = Process.send_after(self(), {:timer_fire, name}, delay_ms)
          entry = %{ref: ref, created_by: "init"}
          %{state | named_timers: Map.put(state.named_timers, name, entry)}
        end

      {:set_timer, name, delay_ms}, state ->
        Logger.warning("Invalid timer delay for #{name} during init: #{inspect(delay_ms)}")
        state

      _other, state ->
        state
    end)
  end

  defp process_effects(effects, state, envelope) do
    context = %{
      actor_id: state.actor_id,
      envelope: envelope,
      manifest: state.manifest,
      server_state: state
    }

    case EffectExecutor.execute(effects, context) do
      %{} = result ->
        {result.reply, result.server_state, result.stop_self, result.sent_ids}

      {:error, :persist_failed, reason} ->
        raise "Persist failed for #{inspect(state.actor_id)}: #{inspect(reason)}"
    end
  end

  defp stash_pending_replies(from, sent_ids, state) do
    timeout_ms = state.manifest.lifecycle.inter_actor_timeout_ms

    Enum.reduce(sent_ids, state, fn msg_id, state ->
      timer_ref = Process.send_after(self(), {:pending_reply_timeout, msg_id}, timeout_ms)
      %{state | pending_replies: Map.put(state.pending_replies, msg_id, {from, timer_ref})}
    end)
  end

  defp pop_pending_reply(envelope, state) do
    case envelope.correlation_id do
      nil ->
        {nil, state}

      corr_id ->
        case Map.pop(state.pending_replies, corr_id) do
          {nil, _} ->
            {nil, state}

          {{from, timer_ref}, pending_replies} ->
            Process.cancel_timer(timer_ref)
            receive do: ({:pending_reply_timeout, ^corr_id} -> :ok), after: (0 -> :ok)
            {from, %{state | pending_replies: pending_replies}}
        end
    end
  end

  defp cancel_pending_replies_for(from, state) do
    {to_cancel, to_keep} =
      Map.split_with(state.pending_replies, fn {_msg_id, {stashed_from, _ref}} ->
        stashed_from == from
      end)

    Enum.each(to_cancel, fn {msg_id, {_from, timer_ref}} ->
      Process.cancel_timer(timer_ref)
      receive do: ({:pending_reply_timeout, ^msg_id} -> :ok), after: (0 -> :ok)
    end)

    %{state | pending_replies: to_keep}
  end

  defp schedule_idle_timer(state) do
    timeout =
      if Quanta.Actor.SubscriberTracker.any_subscribers?(state.actor_id) do
        state.manifest.lifecycle.idle_timeout_ms
      else
        state.manifest.lifecycle.idle_no_subscribers_timeout_ms
      end

    ref = Process.send_after(self(), :passivate, timeout)
    %{state | idle_timer_ref: ref}
  end

  defp reset_idle_timer(state) do
    if state.idle_timer_ref do
      Process.cancel_timer(state.idle_timer_ref)
      receive do: (:passivate -> :ok), after: (0 -> :ok)
    end

    schedule_idle_timer(state)
  end

  defp call_on_passivate(state) do
    if function_exported?(state.module, :on_passivate, 1) do
      state.module.on_passivate(state.state_data)
    end
  end

  defp handle_init_failure(state, error, stacktrace) do
    actor_id = state.actor_id

    :telemetry.execute(
      [:quanta, :actor, :crash],
      %{},
      %{actor_id: actor_id, reason: error, stacktrace: stacktrace}
    )

    if :ets.whereis(@init_attempts_table) != :undefined do
      count =
        try do
          :ets.update_counter(@init_attempts_table, actor_id, {2, 1})
        rescue
          ArgumentError ->
            :ets.insert(@init_attempts_table, {actor_id, 1})
            1
        end

      if count >= 3 do
        Logger.error("Actor #{inspect(actor_id)} failed init 3 times, giving up")
        :ets.delete(@init_attempts_table, actor_id)
        {:stop, :normal, state}
      else
        Logger.error("Actor #{inspect(actor_id)} init failure ##{count}: #{Exception.message(error)}")
        {:stop, {error, stacktrace}, state}
      end
    else
      Logger.error("Actor #{inspect(actor_id)} init failure: #{Exception.message(error)}")
      {:stop, {error, stacktrace}, state}
    end
  end

  defp clear_init_failures(actor_id) do
    if :ets.whereis(@init_attempts_table) != :undefined do
      :ets.delete(@init_attempts_table, actor_id)
    end

    :ok
  rescue
    ArgumentError -> :ok
  end

  defp check_mailbox(state) do
    {:message_queue_len, len} = Process.info(self(), :message_queue_len)

    cond do
      len > @mailbox_critical_threshold ->
        flush_casts()

        :telemetry.execute(
          [:quanta, :actor, :mailbox, :critical],
          %{queue_len: len},
          %{actor_id: state.actor_id}
        )

        :critical

      len > @mailbox_shed_threshold ->
        :telemetry.execute(
          [:quanta, :actor, :mailbox, :shedding],
          %{queue_len: len},
          %{actor_id: state.actor_id}
        )

        :shedding

      len > @mailbox_warn_threshold ->
        :telemetry.execute(
          [:quanta, :actor, :mailbox, :warning],
          %{queue_len: len},
          %{actor_id: state.actor_id}
        )

        :ok

      true ->
        :ok
    end
  end

  defp flush_casts do
    receive do
      {:"$gen_cast", _} -> flush_casts()
    after
      0 -> :ok
    end
  end
end
