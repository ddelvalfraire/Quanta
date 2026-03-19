defmodule Quanta.Actor.CrdtOps do
  @moduledoc false

  alias Quanta.Nifs.LoroEngine

  require Logger

  @spec apply_op(reference(), Quanta.Effect.crdt_op()) :: :ok | {:error, String.t()}
  def apply_op(doc, {:text_insert, cid, pos, text}),
    do: LoroEngine.text_insert(doc, cid, pos, text)

  def apply_op(doc, {:text_delete, cid, pos, len}),
    do: LoroEngine.text_delete(doc, cid, pos, len)

  def apply_op(doc, {:text_mark, cid, from, to, key, value}),
    do: LoroEngine.text_mark(doc, cid, from, to, key, value)

  def apply_op(doc, {:map_set, cid, key, value}),
    do: LoroEngine.map_set(doc, cid, key, value)

  def apply_op(doc, {:map_delete, cid, key}),
    do: LoroEngine.map_delete(doc, cid, key)

  def apply_op(doc, {:list_insert, cid, index, value}),
    do: LoroEngine.list_insert(doc, cid, index, value)

  def apply_op(doc, {:list_delete, cid, index, len}),
    do: LoroEngine.list_delete(doc, cid, index, len)

  def apply_op(doc, {:tree_move, cid, node_id, parent_id}),
    do: LoroEngine.tree_move(doc, cid, node_id, parent_id)

  def apply_op(_doc, op) do
    Logger.warning("Unknown CRDT op: #{inspect(op)}")
    :ok
  end

  @spec apply_ops(reference(), [Quanta.Effect.crdt_op()]) :: :ok
  def apply_ops(doc, ops) do
    Enum.each(ops, &apply_op(doc, &1))
  end

  @spec check_state_size(reference(), pos_integer(), term()) :: :ok
  def check_state_size(doc, max_bytes, actor_id) do
    case LoroEngine.doc_state_size(doc) do
      {:ok, size} when size > max_bytes ->
        Logger.warning(
          "CRDT state size #{size} exceeds max #{max_bytes} for #{inspect(actor_id)}"
        )

      _ ->
        :ok
    end
  end

  @spec encode_value_as_json(reference()) :: {:ok, binary()} | {:error, term()}
  def encode_value_as_json(doc) do
    case LoroEngine.doc_get_value(doc) do
      {:ok, value} ->
        case Jason.encode(value) do
          {:ok, json} -> {:ok, json}
          {:error, _} -> {:ok, Jason.encode!(inspect(value))}
        end

      {:error, reason} ->
        {:error, reason}
    end
  end

  @spec broadcast_update(Quanta.ActorId.t(), binary(), term()) :: :ok
  def broadcast_update(actor_id, delta_bytes, peer_id) do
    group = {:crdt, actor_id}

    members =
      try do
        :pg.get_members(Quanta.Actor.CrdtPubSub, group)
      catch
        :exit, _ -> []
      end

    msg = {:crdt_delta, actor_id, delta_bytes, peer_id}

    for pid <- members, pid != self() do
      send(pid, msg)
    end

    :ok
  end
end
