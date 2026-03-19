defmodule Quanta.Broadway.NatsAcknowledgerTest do
  use ExUnit.Case, async: true

  alias Quanta.Broadway.NatsAcknowledger

  describe "ack/3" do
    test "emits telemetry for successful messages" do
      ref = make_ref()

      :telemetry.attach(
        "test-success-#{inspect(ref)}",
        [:quanta, :broadway, :success],
        fn _event, measurements, _metadata, pid ->
          send(pid, {:telemetry, :success, measurements})
        end,
        self()
      )

      msg = %Broadway.Message{
        data: "test",
        metadata: %{},
        acknowledger: {NatsAcknowledger, :ack_ref, %{}}
      }

      NatsAcknowledger.ack(:ack_ref, [msg, msg], [])
      assert_receive {:telemetry, :success, %{count: 2}}

      :telemetry.detach("test-success-#{inspect(ref)}")
    end

    test "emits telemetry for failed messages" do
      ref = make_ref()

      :telemetry.attach(
        "test-failed-#{inspect(ref)}",
        [:quanta, :broadway, :failed],
        fn _event, measurements, _metadata, pid ->
          send(pid, {:telemetry, :failed, measurements})
        end,
        self()
      )

      msg = %Broadway.Message{
        data: "test",
        metadata: %{},
        acknowledger: {NatsAcknowledger, :ack_ref, %{}}
      }

      NatsAcknowledger.ack(:ack_ref, [], [msg])
      assert_receive {:telemetry, :failed, %{count: 1}}

      :telemetry.detach("test-failed-#{inspect(ref)}")
    end

    test "does not emit telemetry when lists are empty" do
      ref = make_ref()

      :telemetry.attach(
        "test-noop-#{inspect(ref)}",
        [:quanta, :broadway, :success],
        fn _event, _measurements, _metadata, pid ->
          send(pid, :unexpected_telemetry)
        end,
        self()
      )

      NatsAcknowledger.ack(:ack_ref, [], [])
      refute_receive :unexpected_telemetry, 100

      :telemetry.detach("test-noop-#{inspect(ref)}")
    end
  end
end
