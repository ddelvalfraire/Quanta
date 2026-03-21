defmodule Quanta.Actor.Server do
  @moduledoc """
  Actor lifecycle GenServer — activation, message dispatch, passivation.

  In Phase 1 (no WASM), dispatches to an Elixir module implementing
  `Quanta.Actor`. The module is passed via `module:` in start opts.
  """

  use GenServer

  alias Quanta.Actor.{CrdtOps, DynSup, EffectExecutor, ManifestRegistry, Registry}
  alias Quanta.Envelope
  alias Quanta.Nifs.{EphemeralStore, LoroEngine}

  require Logger

  @mailbox_warn_threshold 1_000
  @mailbox_shed_threshold 5_000
  @mailbox_critical_threshold 10_000
  @init_attempts_table :quanta_actor_init_attempts
  @ephemeral_ttl_ms 30_000
  @message_event [:quanta, :actor, :message, :stop]
  @activate_event [:quanta, :actor, :activate, :stop]

  defstruct [
    :actor_id,
    :module,
    :manifest,
    :state_data,
    :idle_timer_ref,
    :activated_at,
    :loro_doc,
    :ephemeral_store,
    :last_version,
    status: :activating,
    events_since_snapshot: 0,
    named_timers: %{},
    pending_replies: %{},
    message_count: 0,
    subscribers: %{},
    rate_count: 0,
    rate_window: 0,
    rate_limit: 1_000,
    last_active_at: 0
  ]

  @type t :: %__MODULE__{
          actor_id: Quanta.ActorId.t() | nil,
          module: module() | nil,
          manifest: Quanta.Manifest.t() | nil,
          state_data: binary() | nil,
          idle_timer_ref: reference() | nil,
          activated_at: integer() | nil,
          loro_doc: reference() | nil,
          ephemeral_store: reference() | nil,
          last_version: binary() | nil,
          status: :activating | :active,
          events_since_snapshot: non_neg_integer(),
          named_timers: %{String.t() => map()},
          pending_replies: %{String.t() => {GenServer.from(), reference()}},
          message_count: non_neg_integer(),
          subscribers: %{pid() => {String.t(), reference()}},
          rate_count: non_neg_integer(),
          rate_window: integer(),
          rate_limit: pos_integer(),
          last_active_at: integer()
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

  @doc "Returns a drain priority (0-4) for ordered passivation. Lower = drain first."
  @spec drain_priority(pid(), timeout()) :: non_neg_integer()
  def drain_priority(pid, timeout \\ 3_000) do
    case :sys.get_state(pid, timeout) do
      %{pending_replies: pr, named_timers: nt, subscribers: subs} ->
        cond do
          map_size(pr) > 0 -> 4
          map_size(nt) > 0 -> 3
          map_size(subs) > 0 -> 2
          true -> 1
        end

      _ ->
        0
    end
  catch
    :exit, _ -> 0
  end

  @spec get_crdt_snapshot(pid()) :: {:ok, binary()} | {:error, :not_crdt | String.t()}
  def get_crdt_snapshot(pid) do
    GenServer.call(pid, :get_crdt_snapshot)
  end

  @spec subscribe(pid(), pid(), String.t()) :: :ok
  def subscribe(pid, channel_pid, user_id) do
    GenServer.call(pid, {:subscribe, channel_pid, user_id})
  end

  @spec unsubscribe(pid(), pid()) :: :ok
  def unsubscribe(pid, channel_pid) do
    GenServer.call(pid, {:unsubscribe, channel_pid})
  end

  @impl true
  def init(opts) do
    actor_id = Keyword.fetch!(opts, :actor_id)
    module = Keyword.fetch!(opts, :module)
    manifest = Keyword.get(opts, :manifest)

    Process.flag(:message_queue_data, :off_heap)

    case Registry.register(actor_id) do
      :ok ->
        Logger.metadata(
          actor_namespace: actor_id.namespace,
          actor_type: actor_id.type,
          actor_id: actor_id.id
        )

        {:ok, %__MODULE__{actor_id: actor_id, module: module},
         {:continue, {:load_state, manifest}}}

      {:error, :already_registered} ->
        {:stop, {:already_registered, actor_id}}
    end
  end

  @impl true
  def handle_continue({:load_state, manifest}, state) when not is_nil(manifest) do
    activate(state, manifest)
  end

  def handle_continue({:load_state, _nil}, state) do
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
    case dispatch_with_backpressure(state, envelope) do
      {:rate_limited, state} ->
        {:reply, {:error, :rate_limited}, state}

      {:overloaded, state} ->
        {:reply, {:error, :overloaded}, state}

      {:ok, reply, state, stop?, sent_ids} ->
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
    reply =
      if state.loro_doc do
        CrdtOps.encode_value_as_json(state.loro_doc)
      else
        {:ok, state.state_data}
      end

    {:reply, reply, state}
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
  def handle_call(:get_crdt_snapshot, _from, state) do
    if state.loro_doc do
      case LoroEngine.doc_export_snapshot(state.loro_doc) do
        {:ok, snapshot} -> {:reply, {:ok, snapshot}, state}
        {:error, reason} -> {:reply, {:error, reason}, state}
      end
    else
      {:reply, {:error, :not_crdt}, state}
    end
  end

  @impl true
  def handle_call({:subscribe, channel_pid, user_id}, _from, state) do
    ref = Process.monitor(channel_pid)
    subscribers = Map.put(state.subscribers, channel_pid, {user_id, ref})
    state = %{state | subscribers: subscribers}
    state = reset_idle_timer(state)

    if state.ephemeral_store do
      {:ok, bytes} = EphemeralStore.encode_all(state.ephemeral_store)
      send(channel_pid, {:ephemeral_state, bytes})
    end

    {:reply, :ok, state}
  end

  @impl true
  def handle_call({:unsubscribe, channel_pid}, _from, state) do
    state = remove_subscriber(state, channel_pid)
    state = reset_idle_timer(state)
    {:reply, :ok, state}
  end

  @impl true
  def handle_cast({:crdt_delta, delta_bytes, peer_id}, state) do
    case LoroEngine.doc_apply_delta(state.loro_doc, delta_bytes) do
      :ok ->
        {:ok, new_version} = LoroEngine.doc_version(state.loro_doc)

        CrdtOps.broadcast_update(state.actor_id, delta_bytes, peer_id)
        broadcast_crdt_update(state, delta_bytes, peer_id)

        state = %{
          state
          | last_version: new_version,
            events_since_snapshot: state.events_since_snapshot + 1
        }

        CrdtOps.check_state_size(
          state.loro_doc,
          state.manifest.state.max_size_bytes,
          state.actor_id
        )

        {:noreply, reset_idle_timer(state)}

      {:error, reason} ->
        Logger.warning("Failed to apply CRDT delta: #{inspect(reason)}")
        {:noreply, state}
    end
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
            reply_to_caller(pending_from, reply)
            cancel_pending_replies_for(pending_from, state)
          else
            state
          end

        if stop?, do: {:stop, :normal, state}, else: {:noreply, state}
    end
  end

  @impl true
  def handle_cast({:ephemeral_update, key, value_bytes, sender_pid}, state) do
    if state.ephemeral_store do
      :ok = EphemeralStore.set(state.ephemeral_store, key, value_bytes)
      {:ok, encoded} = EphemeralStore.encode(state.ephemeral_store, key)
      broadcast_ephemeral(state, encoded, sender_pid)
    end

    {:noreply, state}
  end

  @impl true
  def handle_info(:passivate, state) do
    idle_ms = idle_timeout_ms(state)
    elapsed_ms = System.convert_time_unit(System.monotonic_time() - state.last_active_at, :native, :millisecond)

    if elapsed_ms >= idle_ms do
      :telemetry.execute(
        [:quanta, :actor, :passivate],
        %{},
        %{actor_id: state.actor_id, reason: :idle}
      )

      call_on_passivate(state)
      Registry.deregister(state.actor_id)
      {:stop, :normal, state}
    else
      remaining = idle_ms - elapsed_ms
      ref = Process.send_after(self(), :passivate, remaining)
      {:noreply, %{state | idle_timer_ref: ref}}
    end
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
        reply_to_caller(from, {:error, :actor_timeout})
        {:noreply, %{state | pending_replies: pending_replies}}
    end
  end

  @impl true
  def handle_info({:subscriber_left, user_id}, state) do
    cleanup_ephemeral_for_user(state, user_id)
    {:noreply, reset_idle_timer(state)}
  end

  @impl true
  def handle_info({:DOWN, _ref, :process, pid, _reason}, state) do
    if Map.has_key?(state.subscribers, pid) do
      state = remove_subscriber(state, pid)
      state = reset_idle_timer(state)
      {:noreply, state}
    else
      {:noreply, state}
    end
  end

  @impl true
  def handle_info({:"$quanta_msg", ref, from, envelope}, state) do
    case dispatch_with_backpressure(state, envelope) do
      {:rate_limited, state} ->
        send(from, {:"$quanta_reply", ref, {:error, :rate_limited}})
        {:noreply, state}

      {:overloaded, state} ->
        send(from, {:"$quanta_reply", ref, {:error, :overloaded}})
        {:noreply, state}

      {:ok, reply, state, stop?, sent_ids} ->
        if reply == nil and sent_ids != [] do
          state = stash_quanta_pending_replies({from, ref}, sent_ids, state)
          if stop?, do: {:stop, :normal, state}, else: {:noreply, state}
        else
          result = reply || {:ok, :no_reply}
          send(from, {:"$quanta_reply", ref, result})
          if stop?, do: {:stop, :normal, state}, else: {:noreply, state}
        end
    end
  end

  @impl true
  def handle_info(_msg, state) do
    {:noreply, state}
  end

  defp activate(state, manifest) do
    state = %{state | manifest: manifest, rate_limit: manifest.rate_limits.messages_per_second}
    t0 = System.monotonic_time()

    try do
      state =
        case manifest.state.kind do
          {:crdt, _type} -> activate_crdt(state)
          _ -> activate_standard(state)
        end

      state = schedule_idle_timer(state)
      clear_init_failures(state.actor_id)

      :telemetry.execute(@activate_event,
        %{duration: System.monotonic_time() - t0},
        %{actor_id: state.actor_id})

      {:noreply, state}
    rescue
      e ->
        stacktrace = __STACKTRACE__
        handle_init_failure(state, e, stacktrace)
    end
  end

  defp activate_standard(state) do
    {state_data, init_effects} = state.module.init(<<>>)

    state = %{
      state
      | state_data: state_data,
        status: :active,
        activated_at: System.monotonic_time()
    }

    process_init_effects(init_effects, state)
  end

  defp activate_crdt(state) do
    {:ok, doc} = LoroEngine.doc_new()
    {:ok, tid} = EphemeralStore.new(@ephemeral_ttl_ms)
    {:ok, version} = LoroEngine.doc_version(doc)

    state = %{
      state
      | loro_doc: doc,
        ephemeral_store: tid,
        last_version: version,
        state_data: <<>>,
        status: :active,
        activated_at: System.monotonic_time()
    }

    {_state_data, init_effects} = state.module.init(<<>>)
    process_init_effects(init_effects, state)
  end

  defp dispatch_with_backpressure(state, envelope) do
    now_s = System.monotonic_time(:second)

    state =
      if state.rate_window != now_s,
        do: %{state | rate_window: now_s, rate_count: 0},
        else: state

    cond do
      state.rate_count >= state.rate_limit ->
        :telemetry.execute(
          [:quanta, :rate_limit, :rejected],
          %{},
          %{actor_id: state.actor_id}
        )

        {:rate_limited, state}

      check_mailbox(state) in [:shedding, :critical] ->
        {:overloaded, state}

      true ->
        state = %{state | rate_count: state.rate_count + 1}
        {reply, state, stop?, sent_ids} = dispatch_message(state, envelope)
        {:ok, reply, state, stop?, sent_ids}
    end
  end

  defp dispatch_message(state, envelope) do
    t0 = System.monotonic_time()
    state = %{state | last_active_at: t0}

    result =
      if state.loro_doc do
        dispatch_crdt_message(state, envelope)
      else
        {new_state, effects} = state.module.handle_message(state.state_data, envelope)
        state = %{state | state_data: new_state, message_count: state.message_count + 1}
        process_effects(effects, state, envelope)
      end

    duration = System.monotonic_time() - t0
    :telemetry.execute(@message_event, %{duration: duration},
      %{actor_id: state.actor_id, message_id: envelope.message_id})

    result
  end

  defp dispatch_crdt_message(state, envelope) do
    snapshot_json =
      case CrdtOps.encode_value_as_json(state.loro_doc) do
        {:ok, json} -> json
        {:error, _} -> "{}"
      end

    {_state, effects} = state.module.handle_message(snapshot_json, envelope)
    state = %{state | message_count: state.message_count + 1}
    process_effects(effects, state, envelope)
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

      {:crdt_ops, ops}, state when is_list(ops) and state.loro_doc != nil ->
        CrdtOps.apply_ops(state.loro_doc, ops)
        {:ok, new_version} = LoroEngine.doc_version(state.loro_doc)
        %{state | last_version: new_version, events_since_snapshot: state.events_since_snapshot + 1}

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

  defp stash_quanta_pending_replies({pid, ref}, sent_ids, state) do
    stash_pending_replies({:quanta_direct, pid, ref}, sent_ids, state)
  end

  defp reply_to_caller({:quanta_direct, pid, ref}, reply) do
    send(pid, {:"$quanta_reply", ref, reply})
  end

  defp reply_to_caller(from, reply) do
    GenServer.reply(from, reply)
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

  defp idle_timeout_ms(state) do
    has_subscribers =
      map_size(state.subscribers) > 0 or
        Quanta.Actor.SubscriberTracker.any_subscribers?(state.actor_id)

    if has_subscribers do
      state.manifest.lifecycle.idle_timeout_ms
    else
      state.manifest.lifecycle.idle_no_subscribers_timeout_ms
    end
  end

  defp schedule_idle_timer(state) do
    state = %{state | last_active_at: System.monotonic_time()}
    ref = Process.send_after(self(), :passivate, idle_timeout_ms(state))
    %{state | idle_timer_ref: ref}
  end

  defp reset_idle_timer(state) do
    if state.idle_timer_ref do
      Process.cancel_timer(state.idle_timer_ref)
      receive do: (:passivate -> :ok), after: (0 -> :ok)
    end

    schedule_idle_timer(state)
  end

  defp remove_subscriber(state, channel_pid) do
    case Map.pop(state.subscribers, channel_pid) do
      {nil, _} ->
        state

      {{user_id, ref}, subscribers} ->
        Process.demonitor(ref, [:flush])
        state = %{state | subscribers: subscribers}
        cleanup_ephemeral_for_user(state, user_id)
        state
    end
  end

  defp broadcast_crdt_update(state, delta_bytes, peer_id) do
    msg = {:crdt_update, delta_bytes, peer_id}

    for {pid, {_user_id, _ref}} <- state.subscribers do
      send(pid, msg)
    end

    :ok
  end

  defp cleanup_ephemeral_for_user(state, user_id) do
    if state.ephemeral_store do
      key = "user:#{user_id}"
      :ok = EphemeralStore.delete(state.ephemeral_store, key)
      {:ok, encoded} = EphemeralStore.encode(state.ephemeral_store, key)
      broadcast_ephemeral(state, encoded, nil)
    end
  end

  defp broadcast_ephemeral(state, encoded_bytes, sender_pid) do
    msg = {:ephemeral_update, encoded_bytes, sender_pid}

    for {pid, {_user_id, _ref}} <- state.subscribers do
      send(pid, msg)
    end

    :ok
  end

  defp call_on_passivate(state) do
    if function_exported?(state.module, :on_passivate, 1) do
      passivate_data =
        if state.loro_doc do
          case LoroEngine.doc_export_shallow_snapshot(state.loro_doc) do
            {:ok, snapshot} -> snapshot
            {:error, _} -> state.state_data
          end
        else
          state.state_data
        end

      state.module.on_passivate(passivate_data)
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
