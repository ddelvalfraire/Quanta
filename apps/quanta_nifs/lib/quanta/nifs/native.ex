defmodule Quanta.Nifs.Native do
  @moduledoc """
  Native Rust NIF bindings loaded via Rustler.
  """

  use Rustler,
    otp_app: :quanta_nifs,
    crate: "quanta_nifs",
    path: "../../rust/quanta-nifs"

  @doc "Smoke test: returns true if the NIF is loaded."
  @spec ping() :: boolean()
  def ping(), do: :erlang.nif_error(:nif_not_loaded)

  # --- NATS JetStream ---

  @doc "Connect to NATS server(s). Starts internal Tokio runtime."
  @spec nats_connect(urls :: [String.t()], opts :: map()) ::
          {:ok, reference()} | {:error, String.t()}
  def nats_connect(_urls, _opts), do: :erlang.nif_error(:nif_not_loaded)

  @doc "Publish to a JetStream subject (async, sends result to caller_pid)."
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

  @doc "Get a value from a NATS KV bucket (async, sends result to caller_pid)."
  @spec kv_get_async(
          conn :: reference(),
          caller_pid :: pid(),
          ref :: reference(),
          bucket :: String.t(),
          key :: String.t()
        ) :: :ok | {:error, :nats_backpressure}
  def kv_get_async(_conn, _caller_pid, _ref, _bucket, _key),
    do: :erlang.nif_error(:nif_not_loaded)

  @doc "Put a value to a NATS KV bucket (async, sends result to caller_pid)."
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

  @doc "Delete a key from a NATS KV bucket (async, sends result to caller_pid)."
  @spec kv_delete_async(
          conn :: reference(),
          caller_pid :: pid(),
          ref :: reference(),
          bucket :: String.t(),
          key :: String.t()
        ) :: :ok | {:error, :nats_backpressure}
  def kv_delete_async(_conn, _caller_pid, _ref, _bucket, _key),
    do: :erlang.nif_error(:nif_not_loaded)
end
