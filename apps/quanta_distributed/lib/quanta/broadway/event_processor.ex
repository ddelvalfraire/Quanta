defmodule Quanta.Broadway.EventProcessor do
  @moduledoc """
  Broadway pipeline for processing NATS JetStream events.

  Provides per-actor-id ordering guarantees via processor partitioning.
  Messages are routed to processors based on the actor ID extracted
  from the NATS subject (`quanta.{ns}.evt.{type}.{id}`).

  ## Options

    * `:namespace` — Event namespace (required)
    * `:type` — Event type (required)
    * `:stream_name` — JetStream stream name (required)
    * `:subject_filter` — Subject filter (required)
    * `:processor_concurrency` — Number of processors (default: `System.schedulers_online()`)
    * `:fetch_batch_size` — Max messages per JetStream fetch (default: 256, producer-side)
    * `:batch_size` — Batcher batch size (default: 100)
    * `:batch_timeout` — Batcher timeout in ms (default: 50)
  """

  use Broadway

  require Logger

  alias Quanta.Broadway.NatsProducer

  @default_batcher_batch_size 100
  @default_batcher_timeout 50

  def start_link(opts) do
    namespace = Keyword.fetch!(opts, :namespace)
    type = Keyword.fetch!(opts, :type)
    stream_name = Keyword.fetch!(opts, :stream_name)
    subject_filter = Keyword.fetch!(opts, :subject_filter)
    processor_concurrency = Keyword.get(opts, :processor_concurrency, System.schedulers_online())
    batch_size = Keyword.get(opts, :batch_size, @default_batcher_batch_size)
    batch_timeout = Keyword.get(opts, :batch_timeout, @default_batcher_timeout)

    producer_opts =
      opts
      |> Keyword.take([:fetch_batch_size, :receive_timeout])
      |> Keyword.merge(stream_name: stream_name, subject_filter: subject_filter)

    Broadway.start_link(__MODULE__,
      name: pipeline_name(namespace, type),
      producer: [
        module: {NatsProducer, producer_opts},
        concurrency: 1
      ],
      processors: [
        default: [
          concurrency: processor_concurrency,
          partition_by: &partition_by_actor_id/1
        ]
      ],
      batchers: [
        default: [
          batch_size: batch_size,
          batch_timeout: batch_timeout
        ]
      ]
    )
  end

  @spec pipeline_name(String.t(), String.t()) :: atom()
  def pipeline_name(namespace, type) do
    :"broadway_#{namespace}_#{type}"
  end

  @impl true
  def handle_message(_processor, message, _context) do
    message
  end

  @impl true
  def handle_batch(_batcher, messages, _batch_info, _context) do
    messages
  end

  @impl true
  def handle_failed(messages, _context) do
    Enum.each(messages, fn msg ->
      Logger.error(
        "Broadway message failed: subject=#{msg.metadata[:subject]} seq=#{msg.metadata[:seq]} " <>
          "status=#{inspect(msg.status)}"
      )
    end)

    messages
  end

  def partition_by_actor_id(%Broadway.Message{metadata: %{subject: subject}}) do
    case String.split(subject, ".") do
      ["quanta", _namespace, "evt", _type, actor_id | _] -> :erlang.phash2(actor_id)
      _ -> :erlang.phash2(subject)
    end
  end
end
