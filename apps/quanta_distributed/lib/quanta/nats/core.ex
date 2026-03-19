defmodule Quanta.Nats.Core do
  @moduledoc """
  Convenience wrappers around Gnat for publish, request, subscribe, and unsubscribe.

  Uses a pool of named connections managed by `Quanta.Nats.CoreSupervisor`.
  Publisher connections are selected by hashing the caller PID across the pool.
  Subscriptions always use the first connection (index 0).
  """

  @doc "Return a pool connection name for the calling process (hash-based)."
  @spec connection() :: atom()
  def connection do
    connection(:erlang.phash2(self(), pool_size()))
  end

  @doc "Return the pool connection name at a specific index."
  @spec connection(non_neg_integer()) :: atom()
  def connection(index) when is_integer(index) do
    :"quanta_nats_#{index}"
  end

  @doc "Configured pool size."
  @spec pool_size() :: pos_integer()
  def pool_size do
    Application.get_env(:quanta_distributed, :nats_pool_size, 2)
  end

  @doc "Publish a message to a NATS subject."
  @spec publish(String.t(), binary(), keyword()) :: :ok
  def publish(subject, payload, opts \\ []) do
    Gnat.pub(connection(), subject, payload, opts)
  end

  @doc "Send a request and wait for a reply."
  @spec request(String.t(), binary(), pos_integer()) ::
          {:ok, map()} | {:error, :timeout} | {:error, :no_responders}
  def request(subject, payload, timeout_ms \\ 5_000) do
    Gnat.request(connection(), subject, payload, receive_timeout: timeout_ms)
  end

  @doc """
  Subscribe to a subject, optionally with a queue group.

  Returns `{:ok, {conn_name, sid}}` so the caller can unsubscribe later.
  Always uses pool connection 0 for subscriptions.
  """
  @spec subscribe(String.t(), String.t() | nil, pid()) ::
          {:ok, {atom(), non_neg_integer()}} | {:error, term()}
  def subscribe(subject, queue_group, handler) do
    conn = connection(0)
    opts = if queue_group, do: [queue_group: queue_group], else: []

    case Gnat.sub(conn, handler, subject, opts) do
      {:ok, sid} -> {:ok, {conn, sid}}
      {:error, _} = err -> err
    end
  end

  @doc "Unsubscribe using the `{conn, sid}` tuple returned by `subscribe/3`."
  @spec unsubscribe({atom(), non_neg_integer()}) :: :ok
  def unsubscribe({conn, sid}) do
    Gnat.unsub(conn, sid)
  end

  @doc "Gracefully stop all NATS connections in the pool."
  @spec close_all() :: :ok
  def close_all do
    for i <- 0..(pool_size() - 1) do
      conn = connection(i)

      case Process.whereis(conn) do
        nil -> :ok
        pid -> GenServer.stop(pid, :normal, 5_000)
      end
    end

    :ok
  catch
    kind, reason ->
      require Logger
      Logger.warning("Nats.Core.close_all failed: #{kind} #{inspect(reason)}")
      :ok
  end
end
