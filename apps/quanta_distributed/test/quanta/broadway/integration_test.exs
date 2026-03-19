defmodule Quanta.Broadway.IntegrationTest do
  use ExUnit.Case, async: false

  @moduletag :nats

  import Quanta.Test.NatsHelpers

  alias Quanta.Nats.JetStream

  defmodule TestProcessor do
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
          default: [concurrency: 1]
        ],
        batchers: [
          default: [
            batch_size: 10,
            batch_timeout: 100
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
      send(test_pid, {:batch_processed, Enum.map(messages, & &1.data)})
      messages
    end
  end

  setup do
    stream = "TEST_BROADWAY_#{:erlang.unique_integer([:positive])}"
    subject_prefix = "quanta.test_ns.evt.test_type"
    ensure_stream(stream, "#{subject_prefix}.>")
    on_exit(fn -> delete_stream(stream) end)
    %{stream_name: stream, subject_prefix: subject_prefix}
  end

  test "processes messages from JetStream stream", %{
    stream_name: stream,
    subject_prefix: prefix
  } do
    for i <- 1..5 do
      assert {:ok, _} = JetStream.publish("#{prefix}.actor_#{i}", "event_#{i}")
    end

    name = :"test_broadway_#{:erlang.unique_integer([:positive])}"

    {:ok, _pid} =
      TestProcessor.start_link(
        name: name,
        test_pid: self(),
        stream_name: stream,
        subject_filter: "#{prefix}.>"
      )

    assert_receive {:batch_processed, payloads}, 10_000
    assert length(payloads) == 5

    for i <- 1..5 do
      assert "event_#{i}" in payloads
    end

    Broadway.stop(name)
  end

  test "handles empty stream gracefully", %{stream_name: stream, subject_prefix: prefix} do
    name = :"test_broadway_empty_#{:erlang.unique_integer([:positive])}"

    {:ok, _pid} =
      TestProcessor.start_link(
        name: name,
        test_pid: self(),
        stream_name: stream,
        subject_filter: "#{prefix}.>"
      )

    refute_receive {:batch_processed, _}, 2_000

    Broadway.stop(name)
  end
end
