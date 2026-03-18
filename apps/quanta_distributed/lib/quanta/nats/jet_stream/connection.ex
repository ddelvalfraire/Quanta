defmodule Quanta.Nats.JetStream.Connection do
  @moduledoc """
  GenServer owning the NATS JetStream NIF connection lifecycle.

  Stores the connection reference in `:persistent_term` for near-zero read cost.
  The Rust `async_nats` client handles reconnection internally, so the conn ref
  stays valid across transient disconnects. This GenServer only restarts on
  catastrophic failures.
  """

  use GenServer
  require Logger

  @persistent_term_key :quanta_jetstream_conn
  @max_backoff_ms 5_000

  def start_link(opts \\ []) do
    GenServer.start_link(__MODULE__, opts, name: __MODULE__)
  end

  @doc "Read the connection ref from `:persistent_term`. Raises if not set."
  @spec get_connection() :: reference()
  def get_connection do
    :persistent_term.get(@persistent_term_key)
  end

  @impl true
  def init(_opts) do
    {:ok, %{backoff_ms: 100}, {:continue, :connect}}
  end

  @impl true
  def handle_continue(:connect, state) do
    do_connect(state)
  end

  @impl true
  def handle_info(:retry_connect, state) do
    do_connect(state)
  end

  @impl true
  def terminate(_reason, _state) do
    :persistent_term.erase(@persistent_term_key)
  rescue
    ArgumentError -> :ok
  end

  defp do_connect(state) do
    urls = Application.get_env(:quanta_distributed, :nats_urls, ["nats://localhost:4222"])
    opts = Application.get_env(:quanta_distributed, :jetstream_connect_opts, %{})

    case Quanta.Nifs.Native.nats_connect(urls, opts) do
      {:ok, conn} ->
        :persistent_term.put(@persistent_term_key, conn)
        Logger.info("JetStream connected")
        {:noreply, %{state | backoff_ms: 100}}

      {:error, reason} ->
        backoff = state.backoff_ms
        Logger.warning("JetStream connect failed: #{inspect(reason)}, retrying in #{backoff}ms")
        Process.send_after(self(), :retry_connect, backoff)
        {:noreply, %{state | backoff_ms: min(backoff * 2, @max_backoff_ms)}}
    end
  end
end
