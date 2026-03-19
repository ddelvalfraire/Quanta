defmodule Quanta.Nifs.LoroEngine do
  @moduledoc """
  Elixir wrapper for Loro CRDT NIF operations.

  All functions run on DirtyCpu scheduler. The underlying `LoroDoc` is held in a
  `ResourceArc<Mutex<LoroDoc>>` — access is serialized by the Mutex. The GenServer
  that owns the reference should be the sole accessor to avoid contention.
  """

  alias Quanta.Nifs.Native

  @spec doc_new() :: {:ok, reference()}
  def doc_new, do: Native.loro_doc_new()

  @spec doc_new_with_peer_id(non_neg_integer()) :: {:ok, reference()}
  def doc_new_with_peer_id(peer_id) when is_integer(peer_id) and peer_id >= 0 do
    Native.loro_doc_new_with_peer_id(peer_id)
  end

  @spec doc_import(reference(), binary()) :: :ok | {:error, String.t()}
  def doc_import(doc, bytes) when is_binary(bytes) do
    Native.loro_doc_import(doc, bytes)
  end

  @spec doc_export_snapshot(reference()) :: {:ok, binary()} | {:error, String.t()}
  def doc_export_snapshot(doc), do: Native.loro_doc_export_snapshot(doc)

  @spec doc_export_shallow_snapshot(reference()) :: {:ok, binary()} | {:error, String.t()}
  def doc_export_shallow_snapshot(doc), do: Native.loro_doc_export_shallow_snapshot(doc)

  @doc "Export updates since a given version vector (opaque binary from `doc_version/1`)."
  @spec doc_export_updates_from(reference(), binary()) :: {:ok, binary()} | {:error, String.t()}
  def doc_export_updates_from(doc, version) when is_binary(version) do
    Native.loro_doc_export_updates_from(doc, version)
  end

  @doc "Alias for `doc_import/2`."
  @spec doc_apply_delta(reference(), binary()) :: :ok | {:error, String.t()}
  def doc_apply_delta(doc, delta) when is_binary(delta) do
    doc_import(doc, delta)
  end

  @spec doc_get_value(reference()) :: {:ok, term()} | {:error, String.t()}
  def doc_get_value(doc), do: Native.loro_doc_get_value(doc)

  @spec doc_version(reference()) :: {:ok, binary()} | {:error, String.t()}
  def doc_version(doc), do: Native.loro_doc_version(doc)

  @spec doc_state_size(reference()) :: {:ok, non_neg_integer()} | {:error, String.t()}
  def doc_state_size(doc), do: Native.loro_doc_state_size(doc)

  @spec text_insert(reference(), String.t(), non_neg_integer(), String.t()) ::
          :ok | {:error, String.t()}
  def text_insert(doc, container_id, pos, text)
      when is_binary(container_id) and is_integer(pos) and is_binary(text) do
    Native.loro_text_insert(doc, container_id, pos, text)
  end

  @spec text_delete(reference(), String.t(), non_neg_integer(), non_neg_integer()) ::
          :ok | {:error, String.t()}
  def text_delete(doc, container_id, pos, len)
      when is_binary(container_id) and is_integer(pos) and is_integer(len) do
    Native.loro_text_delete(doc, container_id, pos, len)
  end

  @doc """
  Configure the expand behavior for a text style key.

  Must be called before using `text_mark/6` with that key.
  Expand modes: "after", "before", "both", "none".
  """
  @spec configure_text_style(reference(), String.t(), String.t()) :: :ok | {:error, String.t()}
  def configure_text_style(doc, key, expand)
      when is_binary(key) and is_binary(expand) do
    Native.loro_doc_configure_text_style(doc, key, expand)
  end

  @spec text_mark(reference(), String.t(), non_neg_integer(), non_neg_integer(), String.t(), term()) ::
          :ok | {:error, String.t()}
  def text_mark(doc, container_id, from, to, key, value)
      when is_binary(container_id) and is_integer(from) and is_integer(to) and
             is_binary(key) do
    Native.loro_text_mark(doc, container_id, from, to, key, value)
  end

  @spec text_to_string(reference(), String.t()) :: {:ok, String.t()} | {:error, String.t()}
  def text_to_string(doc, container_id) when is_binary(container_id) do
    Native.loro_text_to_string(doc, container_id)
  end

  @spec text_length(reference(), String.t()) :: {:ok, non_neg_integer()} | {:error, String.t()}
  def text_length(doc, container_id) when is_binary(container_id) do
    Native.loro_text_length(doc, container_id)
  end

  @spec map_set(reference(), String.t(), String.t(), term()) :: :ok | {:error, String.t()}
  def map_set(doc, container_id, key, value)
      when is_binary(container_id) and is_binary(key) do
    Native.loro_map_set(doc, container_id, key, value)
  end

  @spec map_delete(reference(), String.t(), String.t()) :: :ok | {:error, String.t()}
  def map_delete(doc, container_id, key)
      when is_binary(container_id) and is_binary(key) do
    Native.loro_map_delete(doc, container_id, key)
  end

  @spec map_get(reference(), String.t(), String.t()) :: {:ok, term()} | {:error, String.t()}
  def map_get(doc, container_id, key)
      when is_binary(container_id) and is_binary(key) do
    Native.loro_map_get(doc, container_id, key)
  end

  @spec list_insert(reference(), String.t(), non_neg_integer(), term()) ::
          :ok | {:error, String.t()}
  def list_insert(doc, container_id, index, value)
      when is_binary(container_id) and is_integer(index) do
    Native.loro_list_insert(doc, container_id, index, value)
  end

  @spec list_delete(reference(), String.t(), non_neg_integer(), non_neg_integer()) ::
          :ok | {:error, String.t()}
  def list_delete(doc, container_id, index, len)
      when is_binary(container_id) and is_integer(index) and is_integer(len) do
    Native.loro_list_delete(doc, container_id, index, len)
  end

  @spec list_get(reference(), String.t(), non_neg_integer()) ::
          {:ok, term()} | {:error, String.t()}
  def list_get(doc, container_id, index)
      when is_binary(container_id) and is_integer(index) do
    Native.loro_list_get(doc, container_id, index)
  end

  @spec list_length(reference(), String.t()) :: {:ok, non_neg_integer()} | {:error, String.t()}
  def list_length(doc, container_id) when is_binary(container_id) do
    Native.loro_list_length(doc, container_id)
  end

  @spec tree_create_node(reference(), String.t()) :: {:ok, String.t()} | {:error, String.t()}
  def tree_create_node(doc, container_id) when is_binary(container_id) do
    Native.loro_tree_create_node(doc, container_id)
  end

  @doc "Pass `nil` for `parent_id` to move to root."
  @spec tree_move(reference(), String.t(), String.t(), String.t() | nil) ::
          :ok | {:error, String.t()}
  def tree_move(doc, container_id, node_id, parent_id)
      when is_binary(container_id) and is_binary(node_id) and
             (is_binary(parent_id) or is_nil(parent_id)) do
    Native.loro_tree_move(doc, container_id, node_id, parent_id)
  end

  @spec tree_delete(reference(), String.t(), String.t()) :: :ok | {:error, String.t()}
  def tree_delete(doc, container_id, node_id)
      when is_binary(container_id) and is_binary(node_id) do
    Native.loro_tree_delete(doc, container_id, node_id)
  end

  @doc "Side: -1 (left), 0 (middle), 1 (right). Returns opaque cursor binary."
  @spec cursor_at(reference(), String.t(), non_neg_integer(), integer()) ::
          {:ok, binary()} | {:error, String.t()}
  def cursor_at(doc, container_id, pos, side)
      when is_binary(container_id) and is_integer(pos) and is_integer(side) do
    Native.loro_cursor_at(doc, container_id, pos, side)
  end

  @spec cursor_pos(reference(), binary()) :: {:ok, non_neg_integer()} | {:error, String.t()}
  def cursor_pos(doc, cursor) when is_binary(cursor) do
    Native.loro_cursor_pos(doc, cursor)
  end
end
