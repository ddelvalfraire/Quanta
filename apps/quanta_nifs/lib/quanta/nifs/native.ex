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
end
