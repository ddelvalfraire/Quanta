defmodule Quanta.Broadway.NatsProducer do
  @moduledoc """
  GenStage producer that pulls messages from a NATS JetStream consumer.

  Accumulates downstream demand, fetches messages in batches from JetStream,
  and wraps them as `Broadway.Message` structs with the `NatsAcknowledger`.

  ## Options

    * `:stream_name` — JetStream stream name (required)
    * `:subject_filter` — Subject filter for the consumer (required)
    * `:fetch_batch_size` — Max messages per fetch (default: 256)
    * `:receive_timeout` — Fetch timeout in ms (default: 5_000)
  """

  use GenStage

  require Logger

  alias Quanta.Broadway.NatsAcknowledger
  alias Quanta.Nats.JetStream

  @default_batch_size 256
  @default_receive_timeout 5_000
  @retry_interval 1_000

  @impl true
  def init(opts) do
    broadway_opts = Keyword.get(opts, :broadway, [])
    opts = Keyword.merge(opts, broadway_opts)

    stream_name = Keyword.fetch!(opts, :stream_name)
    subject_filter = Keyword.fetch!(opts, :subject_filter)
    batch_size = Keyword.get(opts, :fetch_batch_size, @default_batch_size)
    receive_timeout = Keyword.get(opts, :receive_timeout, @default_receive_timeout)

    case js_impl().consumer_create(stream_name, subject_filter, 0) do
      {:ok, consumer_ref} ->
        state = %{
          demand: 0,
          consumer_ref: consumer_ref,
          stream_name: stream_name,
          batch_size: batch_size,
          receive_timeout: receive_timeout
        }

        {:producer, state}

      {:error, reason} ->
        {:stop, reason}
    end
  end

  @impl true
  def handle_demand(incoming_demand, %{demand: current_demand} = state) do
    new_demand = current_demand + incoming_demand
    send(self(), :fetch)
    {:noreply, [], %{state | demand: new_demand}}
  end

  @impl true
  def handle_info(:fetch, %{demand: 0} = state) do
    {:noreply, [], state}
  end

  def handle_info(:fetch, %{demand: demand} = state) do
    batch = min(demand, state.batch_size)

    case js_impl().consumer_fetch(state.consumer_ref, batch, state.receive_timeout) do
      {:ok, messages} ->
        broadway_msgs = Enum.map(messages, &wrap_message/1)
        new_demand = demand - length(broadway_msgs)

        if new_demand > 0, do: send(self(), :fetch)

        {:noreply, broadway_msgs, %{state | demand: new_demand}}

      {:error, reason} ->
        Logger.warning("NatsProducer fetch failed: #{inspect(reason)}, retrying in #{@retry_interval}ms")
        Process.send_after(self(), :fetch, @retry_interval)
        {:noreply, [], state}
    end
  rescue
    e ->
      Logger.warning("NatsProducer fetch error: #{inspect(e)}, retrying in #{@retry_interval}ms")
      Process.send_after(self(), :fetch, @retry_interval)
      {:noreply, [], state}
  end

  def handle_info(_msg, state) do
    {:noreply, [], state}
  end

  @impl true
  def terminate(_reason, %{consumer_ref: ref}) do
    js_impl().consumer_delete(ref)
    :ok
  rescue
    _ -> :ok
  end

  defp wrap_message(nats_msg) do
    %Broadway.Message{
      data: nats_msg.payload,
      metadata: %{subject: nats_msg.subject, seq: nats_msg.seq},
      acknowledger: {NatsAcknowledger, :ack_ref, %{}}
    }
  end

  defp js_impl, do: JetStream.impl()
end
