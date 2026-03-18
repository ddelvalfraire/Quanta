defmodule Quanta.Nats.JetStream do
  @moduledoc """
  Synchronous Elixir API for NATS JetStream operations.

  Each function calls the async NIF directly from the caller process (no
  GenServer bottleneck), then blocks in `receive` until the NIF sends the
  result back. The connection ref is read from `:persistent_term`.
  """

  @behaviour Quanta.Nats.JetStream.Behaviour

  alias Quanta.Nats.JetStream.Connection
  alias Quanta.Nifs.Native

  @default_timeout 5_000
  # Grace period over the NIF-side fetch timeout so the NIF replies before Elixir gives up
  @consumer_fetch_buffer_ms 2_000

  @doc "Returns the configured implementation module (supports test-time module swap)."
  @spec impl() :: module()
  def impl do
    Application.get_env(:quanta_distributed, :jetstream_impl, __MODULE__)
  end

  @impl true
  def publish(subject, payload, expected_last_subject_seq \\ nil) do
    with {:ok, conn} <- Connection.get_connection() do
      ref = make_ref()

      case Native.js_publish_async(conn, self(), ref, subject, payload, expected_last_subject_seq) do
        :ok -> await_response(ref, @default_timeout)
        {:error, _} = err -> err
      end
    end
  end

  @impl true
  def kv_get(bucket, key) do
    with {:ok, conn} <- Connection.get_connection() do
      ref = make_ref()

      case Native.kv_get_async(conn, self(), ref, bucket, key) do
        :ok ->
          # Reshape NIF map into positional tuple to match @callback
          case await_response(ref, @default_timeout) do
            {:ok, %{value: value, revision: revision}} -> {:ok, value, revision}
            {:error, _} = err -> err
          end

        {:error, _} = err ->
          err
      end
    end
  end

  @impl true
  def kv_put(bucket, key, value) do
    with {:ok, conn} <- Connection.get_connection() do
      ref = make_ref()

      case Native.kv_put_async(conn, self(), ref, bucket, key, value) do
        :ok ->
          # Reshape NIF map into positional tuple to match @callback
          case await_response(ref, @default_timeout) do
            {:ok, %{revision: revision}} -> {:ok, revision}
            {:error, _} = err -> err
          end

        {:error, _} = err ->
          err
      end
    end
  end

  @impl true
  def kv_delete(bucket, key) do
    with {:ok, conn} <- Connection.get_connection() do
      ref = make_ref()

      case Native.kv_delete_async(conn, self(), ref, bucket, key) do
        :ok -> await_response(ref, @default_timeout)
        {:error, _} = err -> err
      end
    end
  end

  @impl true
  def consumer_create(stream, subject_filter, start_seq) do
    with {:ok, conn} <- Connection.get_connection() do
      ref = make_ref()

      case Native.consumer_create_async(conn, self(), ref, stream, subject_filter, start_seq) do
        :ok -> await_response(ref, @default_timeout)
        {:error, _} = err -> err
      end
    end
  end

  @impl true
  def consumer_fetch(consumer, batch_size, timeout_ms) do
    with {:ok, conn} <- Connection.get_connection() do
      ref = make_ref()

      case Native.consumer_fetch_async(conn, self(), ref, consumer, batch_size, timeout_ms) do
        :ok -> await_response(ref, timeout_ms + @consumer_fetch_buffer_ms)
        {:error, _} = err -> err
      end
    end
  end

  @impl true
  def consumer_delete(consumer) do
    with {:ok, conn} <- Connection.get_connection() do
      ref = make_ref()

      case Native.consumer_delete_async(conn, self(), ref, consumer) do
        :ok -> await_response(ref, @default_timeout)
        {:error, _} = err -> err
      end
    end
  end

  @impl true
  def purge_subject(stream, subject) do
    with {:ok, conn} <- Connection.get_connection() do
      ref = make_ref()

      case Native.purge_subject_async(conn, self(), ref, stream, subject) do
        :ok -> await_response(ref, @default_timeout)
        {:error, _} = err -> err
      end
    end
  end

  # --- Private helpers ---

  defp await_response(ref, timeout) do
    receive do
      {:ok, ^ref, result} -> {:ok, result}
      {:ok, ^ref} -> :ok
      {:error, ^ref, reason} -> {:error, reason}
    after
      timeout ->
        drain(ref)
        {:error, :timeout}
    end
  end

  defp drain(ref) do
    receive do
      {:ok, ^ref, _} -> :ok
      {:ok, ^ref} -> :ok
      {:error, ^ref, _} -> :ok
    after
      0 -> :ok
    end
  end
end
