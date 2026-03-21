defmodule Quanta.Actor.EffectExecutor do
  @moduledoc """
  Stateless effect executor for actor side effects.

  Processes a list of effects in order, accumulating state changes,
  reply values, and sent message IDs. Persist failure halts execution.
  """

  alias Quanta.Actor.{CrdtOps, DynSup, Registry, Server}
  alias Quanta.Codec.Wire
  alias Quanta.Envelope
  alias Quanta.Nifs.{DeltaEncoder, LoroEngine}

  require Logger

  @type context :: %{
          actor_id: Quanta.ActorId.t(),
          envelope: Envelope.t(),
          manifest: Quanta.Manifest.t(),
          server_state: Server.t(),
          prev_state_data: binary() | nil
        }

  # Deviations from ticket spec (T12):
  # - `reply` wraps in `{:ok, binary()}` (ticket says `binary() | nil`) because
  #   Server passes it directly as the GenServer reply to `send_message/3`.
  # - `sent_ids` added for Server's pending-reply correlation mechanism.
  @type result :: %{
          reply: {:ok, binary()} | nil,
          server_state: Server.t(),
          stop_self: boolean(),
          sent_ids: [String.t()]
        }

  @spec execute([Quanta.Effect.t()], context()) :: result() | {:error, :persist_failed, term()}
  def execute(effects, context) do
    initial = %{
      reply: nil,
      server_state: context.server_state,
      stop_self: false,
      sent_ids: []
    }

    Enum.reduce_while(effects, initial, fn effect, acc ->
      case execute_one(effect, acc, context) do
        {:error, :persist_failed, _} = error -> {:halt, error}
        acc -> {:cont, acc}
      end
    end)
  end

  ## Effect handlers

  defp execute_one({:reply, payload}, acc, _context) do
    if acc.reply do
      Logger.warning("Multiple :reply effects — keeping first, ignoring subsequent")
      acc
    else
      %{acc | reply: {:ok, payload}}
    end
  end

  defp execute_one({:persist, data}, acc, context) do
    max = context.manifest.state.max_size_bytes

    if byte_size(data) > max do
      {:error, :persist_failed, :state_too_large}
    else
      state = acc.server_state
      old_state_data = context[:prev_state_data] || state.state_data

      new_seq = state.delta_seq + 1

      new_state = %{
        state
        | state_data: data,
          events_since_snapshot: state.events_since_snapshot + 1,
          delta_seq: new_seq
      }

      broadcast_delta(new_state, old_state_data, data, new_seq)
      %{acc | server_state: new_state}
    end
  end

  defp execute_one({:send, target, payload}, acc, context) do
    out_envelope = build_outgoing_envelope(payload, context)

    case Registry.lookup(target) do
      {:ok, pid} ->
        GenServer.cast(pid, {:incoming_message, out_envelope})
        %{acc | sent_ids: [out_envelope.message_id | acc.sent_ids]}

      :not_found ->
        subject = "quanta.#{target.namespace}.cmd.#{target.type}.#{target.id}"

        case safe_nats_publish(subject, Wire.encode(out_envelope)) do
          :ok -> %{acc | sent_ids: [out_envelope.message_id | acc.sent_ids]}
          :error -> acc
        end
    end
  end

  defp execute_one({:publish, channel, payload}, acc, context) do
    subject = "quanta.#{context.actor_id.namespace}.pub.#{channel}"
    safe_nats_publish(subject, payload)
    acc
  end

  defp execute_one({:set_timer, name, delay_ms}, acc, context)
       when is_integer(delay_ms) and delay_ms > 0 do
    state = acc.server_state
    max = context.manifest.resources.max_timers

    if map_size(state.named_timers) >= max do
      Logger.warning(
        "Max timers (#{max}) reached for #{inspect(context.actor_id)}, dropping timer #{name}"
      )

      acc
    else
      state = cancel_named_timer(state, name)
      ref = Process.send_after(self(), {:timer_fire, name}, delay_ms)
      entry = %{ref: ref, created_by: context.envelope.message_id}
      %{acc | server_state: %{state | named_timers: Map.put(state.named_timers, name, entry)}}
    end
  end

  defp execute_one({:set_timer, name, delay_ms}, acc, _context) do
    Logger.warning(
      "Invalid timer delay for #{name}: #{inspect(delay_ms)}, must be positive integer"
    )

    acc
  end

  defp execute_one({:cancel_timer, name}, acc, _context) do
    %{acc | server_state: cancel_named_timer(acc.server_state, name)}
  end

  defp execute_one({:emit_telemetry, event, measurements, metadata}, acc, _context) do
    event_atoms = event |> String.split(".") |> Enum.map(&safe_to_existing_atom/1)

    if Enum.all?(event_atoms, &is_atom/1) do
      :telemetry.execute([:quanta, :actor, :custom | event_atoms], measurements, metadata)
    else
      Logger.warning("Unknown telemetry event segment in #{inspect(event)}, dropping event")
    end

    acc
  end

  # Phase 1: spawned actors inherit the parent's module. T06 will resolve
  # the dispatch target from the manifest via WASM component lookup.
  defp execute_one({:spawn_actor, target, _init_payload}, acc, context) do
    opts = [actor_id: target, module: context.server_state.module]

    case DynSup.start_actor(target, child_spec: {Server, opts}) do
      {:ok, _pid} -> :ok
      {:error, {:already_started, _pid}} -> Logger.info("Actor #{inspect(target)} already running, ignoring :spawn_actor")
      {:error, reason} -> Logger.warning("Failed to spawn #{inspect(target)}: #{inspect(reason)}")
    end

    acc
  end

  defp execute_one(:stop_self, acc, _context) do
    %{acc | stop_self: true}
  end

  # Phase 1: MFA is unconstrained because actor modules are trusted Elixir code.
  # T06 (WASM) must restrict this to an allowlist before untrusted code runs.
  defp execute_one({:side_effect, {m, f, a}}, acc, _context) do
    Task.Supervisor.start_child(Quanta.SideEffect.TaskSupervisor, fn ->
      apply(m, f, a)
    end)

    acc
  end

  defp execute_one({:crdt_ops, ops}, acc, context) when is_list(ops) do
    state = acc.server_state
    doc = state.loro_doc

    if is_nil(doc) do
      Logger.warning("crdt_ops effect on non-CRDT actor #{inspect(context.actor_id)}, ignoring")
      acc
    else
      CrdtOps.apply_ops(doc, ops)

      case LoroEngine.doc_export_updates_from(doc, state.last_version) do
        {:ok, delta} ->
          {:ok, new_version} = LoroEngine.doc_version(doc)

          CrdtOps.broadcast_update(context.actor_id, delta, nil)
          notify_subscribers(state.subscribers, delta, nil)

          new_state = %{
            state
            | last_version: new_version,
              events_since_snapshot: state.events_since_snapshot + 1
          }

          CrdtOps.check_state_size(doc, context.manifest.state.max_size_bytes, context.actor_id)
          %{acc | server_state: new_state}

        {:error, reason} ->
          Logger.warning("Failed to export CRDT delta: #{inspect(reason)}")
          acc
      end
    end
  end

  ## Helpers

  defp build_outgoing_envelope(payload, context) do
    Envelope.new(
      timestamp: Quanta.HLC.Server.now(),
      causation_id: context.envelope.message_id,
      correlation_id: context.envelope.message_id,
      sender: context.actor_id,
      payload: payload
    )
  end

  defp cancel_named_timer(state, name) do
    case Map.pop(state.named_timers, name) do
      {nil, _} ->
        state

      {entry, named_timers} ->
        Process.cancel_timer(entry.ref)
        %{state | named_timers: named_timers}
    end
  end

  defp safe_to_existing_atom(string) do
    String.to_existing_atom(string)
  rescue
    ArgumentError -> {:error, string}
  end

  defp notify_subscribers(subscribers, delta_bytes, peer_id) do
    msg = {:crdt_update, delta_bytes, peer_id}

    for {pid, {_user_id, _ref}} <- subscribers do
      send(pid, msg)
    end

    :ok
  end

  defp broadcast_delta(state, old_data, new_data, seq) do
    if state.schema_ref && map_size(state.subscribers) > 0 do
      case DeltaEncoder.compute_delta(state.schema_ref, old_data, new_data) do
        {:ok, delta} when byte_size(delta) > 0 ->
          msg = {:delta_update, delta, new_data, seq, state.schema_version}

          for {pid, {_user_id, _ref}} <- state.subscribers do
            send(pid, msg)
          end

        _ ->
          :ok
      end
    end

    :ok
  end

  # TODO: §22.1 requires `:send` via NATS to retry 3x with exponential backoff
  # (100ms, 500ms, 2s) before dropping. Currently fire-and-forget.
  @spec safe_nats_publish(String.t(), binary()) :: :ok | :error
  defp safe_nats_publish(subject, payload) do
    Quanta.Nats.Core.publish(subject, payload)
  rescue
    e ->
      Logger.warning("NATS publish to #{subject} failed: #{Exception.message(e)}")
      :error
  catch
    :exit, reason ->
      Logger.warning("NATS publish to #{subject} failed: #{inspect(reason)}")
      :error
  end
end
