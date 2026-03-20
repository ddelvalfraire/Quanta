defmodule Quanta.Nifs.Native do
  @moduledoc """
  Low-level Rust NIF bindings. All async NIFs return `:ok` immediately and
  send `{:ok, ref, result}` or `{:error, ref, reason}` to the caller PID.
  Backpressure is enforced via a semaphore — when full, returns
  `{:error, :nats_backpressure}` without spawning a task.
  """

  use Rustler,
    otp_app: :quanta_nifs,
    crate: "quanta_nifs",
    path: "../../rust/quanta-nifs"

  @spec ping() :: boolean()
  def ping(), do: :erlang.nif_error(:nif_not_loaded)

  @spec encode_envelope_header(map()) :: {:ok, binary()} | {:error, String.t()}
  def encode_envelope_header(_header), do: :erlang.nif_error(:nif_not_loaded)

  @spec decode_envelope_header(binary()) :: {:ok, map()} | {:error, String.t()}
  def decode_envelope_header(_binary), do: :erlang.nif_error(:nif_not_loaded)

  # --- WasmRuntime: engine / component / linker ---

  def engine_new(), do: :erlang.nif_error(:nif_not_loaded)
  def component_compile(_engine, _wasm_bytes), do: :erlang.nif_error(:nif_not_loaded)
  def linker_new(_engine), do: :erlang.nif_error(:nif_not_loaded)
  def component_serialize(_component, _hmac_key), do: :erlang.nif_error(:nif_not_loaded)
  def component_deserialize(_engine, _bytes, _hmac_key), do: :erlang.nif_error(:nif_not_loaded)
  def call_init(_engine, _component, _linker, _payload, _fuel, _mem),
    do: :erlang.nif_error(:nif_not_loaded)
  def call_handle_message(_engine, _component, _linker, _state, _envelope, _fuel, _mem),
    do: :erlang.nif_error(:nif_not_loaded)
  def call_handle_timer(_engine, _component, _linker, _state, _timer_name, _fuel, _mem),
    do: :erlang.nif_error(:nif_not_loaded)
  def call_migrate(_engine, _component, _linker, _state, _from_version, _fuel, _mem),
    do: :erlang.nif_error(:nif_not_loaded)
  def call_on_passivate(_engine, _component, _linker, _state, _fuel, _mem),
    do: :erlang.nif_error(:nif_not_loaded)

  # --- NATS JetStream ---

  @spec nats_connect(urls :: [String.t()], opts :: map()) ::
          {:ok, reference()} | {:error, String.t()}
  def nats_connect(_urls, _opts), do: :erlang.nif_error(:nif_not_loaded)

  @spec js_publish_async(
          conn :: reference(),
          caller_pid :: pid(),
          ref :: reference(),
          subject :: String.t(),
          payload :: binary(),
          expected_last_subject_seq :: non_neg_integer() | nil
        ) :: :ok | {:error, :nats_backpressure}
  def js_publish_async(_conn, _caller_pid, _ref, _subject, _payload, _expected_last_subject_seq),
    do: :erlang.nif_error(:nif_not_loaded)

  @spec kv_get_async(
          conn :: reference(),
          caller_pid :: pid(),
          ref :: reference(),
          bucket :: String.t(),
          key :: String.t()
        ) :: :ok | {:error, :nats_backpressure}
  def kv_get_async(_conn, _caller_pid, _ref, _bucket, _key),
    do: :erlang.nif_error(:nif_not_loaded)

  @spec kv_put_async(
          conn :: reference(),
          caller_pid :: pid(),
          ref :: reference(),
          bucket :: String.t(),
          key :: String.t(),
          value :: binary()
        ) :: :ok | {:error, :nats_backpressure}
  def kv_put_async(_conn, _caller_pid, _ref, _bucket, _key, _value),
    do: :erlang.nif_error(:nif_not_loaded)

  @spec kv_delete_async(
          conn :: reference(),
          caller_pid :: pid(),
          ref :: reference(),
          bucket :: String.t(),
          key :: String.t()
        ) :: :ok | {:error, :nats_backpressure}
  def kv_delete_async(_conn, _caller_pid, _ref, _bucket, _key),
    do: :erlang.nif_error(:nif_not_loaded)

  # --- Consumer lifecycle ---

  @spec consumer_create_async(
          conn :: reference(),
          caller_pid :: pid(),
          ref :: reference(),
          stream :: String.t(),
          subject_filter :: String.t(),
          start_seq :: non_neg_integer()
        ) :: :ok | {:error, :nats_backpressure}
  def consumer_create_async(_conn, _caller_pid, _ref, _stream, _subject_filter, _start_seq),
    do: :erlang.nif_error(:nif_not_loaded)

  @spec consumer_fetch_async(
          conn :: reference(),
          caller_pid :: pid(),
          ref :: reference(),
          consumer :: reference(),
          batch_size :: pos_integer(),
          timeout_ms :: pos_integer()
        ) :: :ok | {:error, :nats_backpressure}
  def consumer_fetch_async(_conn, _caller_pid, _ref, _consumer, _batch_size, _timeout_ms),
    do: :erlang.nif_error(:nif_not_loaded)

  @spec consumer_delete_async(
          conn :: reference(),
          caller_pid :: pid(),
          ref :: reference(),
          consumer :: reference()
        ) :: :ok | {:error, :nats_backpressure}
  def consumer_delete_async(_conn, _caller_pid, _ref, _consumer),
    do: :erlang.nif_error(:nif_not_loaded)

  # --- Stream management ---

  @spec purge_subject_async(
          conn :: reference(),
          caller_pid :: pid(),
          ref :: reference(),
          stream :: String.t(),
          subject :: String.t()
        ) :: :ok | {:error, :nats_backpressure}
  def purge_subject_async(_conn, _caller_pid, _ref, _stream, _subject),
    do: :erlang.nif_error(:nif_not_loaded)

  # --- Loro CRDT Engine ---

  # Document lifecycle

  @spec loro_doc_new() :: {:ok, reference()}
  def loro_doc_new(), do: :erlang.nif_error(:nif_not_loaded)

  @spec loro_doc_new_with_peer_id(peer_id :: non_neg_integer()) :: {:ok, reference()}
  def loro_doc_new_with_peer_id(_peer_id), do: :erlang.nif_error(:nif_not_loaded)

  @spec loro_doc_import(doc :: reference(), bytes :: binary()) :: :ok | {:error, String.t()}
  def loro_doc_import(_doc, _bytes), do: :erlang.nif_error(:nif_not_loaded)

  @spec loro_doc_export_snapshot(doc :: reference()) :: {:ok, binary()} | {:error, String.t()}
  def loro_doc_export_snapshot(_doc), do: :erlang.nif_error(:nif_not_loaded)

  @spec loro_doc_export_shallow_snapshot(doc :: reference()) ::
          {:ok, binary()} | {:error, String.t()}
  def loro_doc_export_shallow_snapshot(_doc), do: :erlang.nif_error(:nif_not_loaded)

  @spec loro_doc_export_updates_from(doc :: reference(), version :: binary()) ::
          {:ok, binary()} | {:error, String.t()}
  def loro_doc_export_updates_from(_doc, _version), do: :erlang.nif_error(:nif_not_loaded)

  @spec loro_doc_get_value(doc :: reference()) :: {:ok, term()} | {:error, String.t()}
  def loro_doc_get_value(_doc), do: :erlang.nif_error(:nif_not_loaded)

  @spec loro_doc_version(doc :: reference()) :: {:ok, binary()} | {:error, String.t()}
  def loro_doc_version(_doc), do: :erlang.nif_error(:nif_not_loaded)

  @spec loro_doc_state_size(doc :: reference()) :: {:ok, non_neg_integer()} | {:error, String.t()}
  def loro_doc_state_size(_doc), do: :erlang.nif_error(:nif_not_loaded)

  # Text container

  @spec loro_text_insert(
          doc :: reference(),
          container_id :: String.t(),
          pos :: non_neg_integer(),
          text :: String.t()
        ) :: :ok | {:error, String.t()}
  def loro_text_insert(_doc, _container_id, _pos, _text),
    do: :erlang.nif_error(:nif_not_loaded)

  @spec loro_text_delete(
          doc :: reference(),
          container_id :: String.t(),
          pos :: non_neg_integer(),
          len :: non_neg_integer()
        ) :: :ok | {:error, String.t()}
  def loro_text_delete(_doc, _container_id, _pos, _len),
    do: :erlang.nif_error(:nif_not_loaded)

  @spec loro_doc_configure_text_style(
          doc :: reference(),
          key :: String.t(),
          expand :: String.t()
        ) :: :ok | {:error, String.t()}
  def loro_doc_configure_text_style(_doc, _key, _expand),
    do: :erlang.nif_error(:nif_not_loaded)

  @spec loro_text_mark(
          doc :: reference(),
          container_id :: String.t(),
          from :: non_neg_integer(),
          to :: non_neg_integer(),
          key :: String.t(),
          value :: term()
        ) :: :ok | {:error, String.t()}
  def loro_text_mark(_doc, _container_id, _from, _to, _key, _value),
    do: :erlang.nif_error(:nif_not_loaded)

  @spec loro_text_to_string(doc :: reference(), container_id :: String.t()) ::
          {:ok, String.t()} | {:error, String.t()}
  def loro_text_to_string(_doc, _container_id),
    do: :erlang.nif_error(:nif_not_loaded)

  @spec loro_text_length(doc :: reference(), container_id :: String.t()) ::
          {:ok, non_neg_integer()} | {:error, String.t()}
  def loro_text_length(_doc, _container_id),
    do: :erlang.nif_error(:nif_not_loaded)

  # Map container

  @spec loro_map_set(
          doc :: reference(),
          container_id :: String.t(),
          key :: String.t(),
          value :: term()
        ) :: :ok | {:error, String.t()}
  def loro_map_set(_doc, _container_id, _key, _value),
    do: :erlang.nif_error(:nif_not_loaded)

  @spec loro_map_delete(doc :: reference(), container_id :: String.t(), key :: String.t()) ::
          :ok | {:error, String.t()}
  def loro_map_delete(_doc, _container_id, _key),
    do: :erlang.nif_error(:nif_not_loaded)

  @spec loro_map_get(doc :: reference(), container_id :: String.t(), key :: String.t()) ::
          {:ok, term()} | {:error, String.t()}
  def loro_map_get(_doc, _container_id, _key),
    do: :erlang.nif_error(:nif_not_loaded)

  # List container

  @spec loro_list_insert(
          doc :: reference(),
          container_id :: String.t(),
          index :: non_neg_integer(),
          value :: term()
        ) :: :ok | {:error, String.t()}
  def loro_list_insert(_doc, _container_id, _index, _value),
    do: :erlang.nif_error(:nif_not_loaded)

  @spec loro_list_delete(
          doc :: reference(),
          container_id :: String.t(),
          index :: non_neg_integer(),
          len :: non_neg_integer()
        ) :: :ok | {:error, String.t()}
  def loro_list_delete(_doc, _container_id, _index, _len),
    do: :erlang.nif_error(:nif_not_loaded)

  @spec loro_list_get(
          doc :: reference(),
          container_id :: String.t(),
          index :: non_neg_integer()
        ) :: {:ok, term()} | {:error, String.t()}
  def loro_list_get(_doc, _container_id, _index),
    do: :erlang.nif_error(:nif_not_loaded)

  @spec loro_list_length(doc :: reference(), container_id :: String.t()) ::
          {:ok, non_neg_integer()} | {:error, String.t()}
  def loro_list_length(_doc, _container_id),
    do: :erlang.nif_error(:nif_not_loaded)

  # Tree container

  @spec loro_tree_create_node(doc :: reference(), container_id :: String.t()) ::
          {:ok, String.t()} | {:error, String.t()}
  def loro_tree_create_node(_doc, _container_id),
    do: :erlang.nif_error(:nif_not_loaded)

  @spec loro_tree_move(
          doc :: reference(),
          container_id :: String.t(),
          node_id :: String.t(),
          parent_id :: String.t() | nil
        ) :: :ok | {:error, String.t()}
  def loro_tree_move(_doc, _container_id, _node_id, _parent_id),
    do: :erlang.nif_error(:nif_not_loaded)

  @spec loro_tree_delete(
          doc :: reference(),
          container_id :: String.t(),
          node_id :: String.t()
        ) :: :ok | {:error, String.t()}
  def loro_tree_delete(_doc, _container_id, _node_id),
    do: :erlang.nif_error(:nif_not_loaded)

  # Cursor

  @spec loro_cursor_at(
          doc :: reference(),
          container_id :: String.t(),
          pos :: non_neg_integer(),
          side :: integer()
        ) :: {:ok, binary()} | {:error, String.t()}
  def loro_cursor_at(_doc, _container_id, _pos, _side),
    do: :erlang.nif_error(:nif_not_loaded)

  @spec loro_cursor_pos(doc :: reference(), cursor :: binary()) ::
          {:ok, non_neg_integer()} | {:error, String.t()}
  def loro_cursor_pos(_doc, _cursor),
    do: :erlang.nif_error(:nif_not_loaded)

  # --- Loro EphemeralStore ---

  @spec ephemeral_store_new(timeout_ms :: non_neg_integer()) :: {:ok, reference()}
  def ephemeral_store_new(_timeout_ms), do: :erlang.nif_error(:nif_not_loaded)

  @spec ephemeral_store_set(store :: reference(), key :: String.t(), value :: binary()) :: :ok
  def ephemeral_store_set(_store, _key, _value), do: :erlang.nif_error(:nif_not_loaded)

  @spec ephemeral_store_get(store :: reference(), key :: String.t()) ::
          {:ok, binary()} | :not_found
  def ephemeral_store_get(_store, _key), do: :erlang.nif_error(:nif_not_loaded)

  @spec ephemeral_store_delete(store :: reference(), key :: String.t()) :: :ok
  def ephemeral_store_delete(_store, _key), do: :erlang.nif_error(:nif_not_loaded)

  @spec ephemeral_store_get_all(store :: reference()) :: {:ok, %{String.t() => binary()}}
  def ephemeral_store_get_all(_store), do: :erlang.nif_error(:nif_not_loaded)

  @spec ephemeral_store_keys(store :: reference()) :: {:ok, [String.t()]}
  def ephemeral_store_keys(_store), do: :erlang.nif_error(:nif_not_loaded)

  @spec ephemeral_store_encode(store :: reference(), key :: String.t()) :: {:ok, binary()}
  def ephemeral_store_encode(_store, _key), do: :erlang.nif_error(:nif_not_loaded)

  @spec ephemeral_store_encode_all(store :: reference()) :: {:ok, binary()}
  def ephemeral_store_encode_all(_store), do: :erlang.nif_error(:nif_not_loaded)

  @spec ephemeral_store_apply_encoded(store :: reference(), bytes :: binary()) ::
          :ok | {:error, String.t()}
  def ephemeral_store_apply_encoded(_store, _bytes), do: :erlang.nif_error(:nif_not_loaded)

  # --- Schema Compiler ---

  @spec schema_compile(wit_source :: String.t(), type_name :: String.t()) ::
          {:ok, reference(), [String.t()]} | {:error, String.t()}
  def schema_compile(_wit_source, _type_name), do: :erlang.nif_error(:nif_not_loaded)

  @spec schema_export(schema :: reference()) :: {:ok, binary()}
  def schema_export(_schema), do: :erlang.nif_error(:nif_not_loaded)
end
