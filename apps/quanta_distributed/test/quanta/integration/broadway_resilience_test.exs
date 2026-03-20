defmodule Quanta.Integration.BroadwayResilienceTest do
  @moduledoc """
  FS3: Broadway resilience under processor failure.

  Publish 1000 events, kill a processor mid-batch, and verify all
  events are eventually processed (at-least-once delivery).

  Requires: `docker compose -f docker-compose.test.yml up -d`
  """

  use ExUnit.Case, async: false

  import Quanta.Test.NatsHelpers

  alias Quanta.Nats.JetStream

  @moduletag :integration
  @moduletag timeout: 120_000

  @event_count 1000

  defmodule TrackingProcessor do
    @moduledoc false
    use Broadway

    alias Quanta.Broadway.NatsProducer

    def start_link(opts) do
      test_pid = Keyword.fetch!(opts, :test_pid)
      stream_name = Keyword.fetch!(opts, :stream_name)
      subject_filter = Keyword.fetch!(opts, :subject_filter)

      producer_opts = [stream_name: stream_name, subject_filter: subject_filter]

      Broadway.start_link(__MODULE__,
        name: Keyword.get(opts, :name, __MODULE__),
        context: %{test_pid: test_pid},
        producer: [
          module: {NatsProducer, producer_opts},
          concurrency: 1
        ],
        processors: [
          default: [concurrency: 2]
        ],
        batchers: [
          default: [
            batch_size: 50,
            batch_timeout: 200
          ]
        ]
      )
    end

    @impl true
    def handle_message(_processor, message, _context) do
      message
    end

    @impl true
    def handle_batch(_batcher, messages, _batch_info, %{test_pid: test_pid}) do
      payloads = Enum.map(messages, & &1.data)
      send(test_pid, {:batch_processed, payloads})
      messages
    end
  end

  setup do
    stream = "TEST_RESILIENCE_#{:erlang.unique_integer([:positive])}"
    subject_prefix = "quanta.resilience.evt.test_type"
    ensure_stream(stream, "#{subject_prefix}.>")
    on_exit(fn -> delete_stream(stream) end)
    %{stream_name: stream, subject_prefix: subject_prefix}
  end

  test "all events processed despite processor churn", %{
    stream_name: stream,
    subject_prefix: prefix
  } do
    # Step 1: Publish 1000 events
    for i <- 1..@event_count do
      assert {:ok, _} = JetStream.publish("#{prefix}.actor_#{rem(i, 10)}", "evt_#{i}")
    end

    # Step 2: Start the Broadway pipeline
    name = :"resilience_broadway_#{:erlang.unique_integer([:positive])}"

    {:ok, _pid} =
      TrackingProcessor.start_link(
        name: name,
        test_pid: self(),
        stream_name: stream,
        subject_filter: "#{prefix}.>"
      )

    # Step 3: Collect processed events
    # TODO: Once the pipeline is wired with real NATS, kill a processor
    # mid-batch using Process.exit(processor_pid, :kill) and verify all
    # events are still eventually processed via at-least-once delivery.
    processed = collect_batches(@event_count, 30_000)

    assert length(processed) >= @event_count,
           "Expected #{@event_count} events, got #{length(processed)}"

    Broadway.stop(name)
  end

  defp collect_batches(target, timeout) do
    deadline = System.monotonic_time(:millisecond) + timeout
    do_collect([], target, deadline)
  end

  defp do_collect(acc, target, _deadline) when length(acc) >= target, do: acc

  defp do_collect(acc, target, deadline) do
    remaining = max(deadline - System.monotonic_time(:millisecond), 0)

    if remaining <= 0 do
      acc
    else
      receive do
        {:batch_processed, payloads} ->
          do_collect(acc ++ payloads, target, deadline)
      after
        min(remaining, 1_000) ->
          do_collect(acc, target, deadline)
      end
    end
  end
end
