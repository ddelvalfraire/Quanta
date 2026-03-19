defmodule Quanta.Broadway.EventProcessorTest do
  use ExUnit.Case, async: true

  alias Quanta.Broadway.EventProcessor

  describe "partition_by_actor_id/1" do
    test "extracts actor_id and hashes it" do
      msg = %Broadway.Message{
        data: "test",
        metadata: %{subject: "quanta.myns.evt.mytype.actor123"},
        acknowledger: {Quanta.Broadway.NatsAcknowledger, :ack_ref, %{}}
      }

      result = EventProcessor.partition_by_actor_id(msg)
      assert result == :erlang.phash2("actor123")
    end

    test "same actor_id always maps to same partition" do
      make_msg = fn id ->
        %Broadway.Message{
          data: "test",
          metadata: %{subject: "quanta.ns.evt.type.#{id}"},
          acknowledger: {Quanta.Broadway.NatsAcknowledger, :ack_ref, %{}}
        }
      end

      result1 = EventProcessor.partition_by_actor_id(make_msg.("abc"))
      result2 = EventProcessor.partition_by_actor_id(make_msg.("abc"))
      result3 = EventProcessor.partition_by_actor_id(make_msg.("xyz"))

      assert result1 == result2
      assert is_integer(result3)
    end

    test "falls back to full subject hash for unexpected format" do
      msg = %Broadway.Message{
        data: "test",
        metadata: %{subject: "unexpected.format"},
        acknowledger: {Quanta.Broadway.NatsAcknowledger, :ack_ref, %{}}
      }

      result = EventProcessor.partition_by_actor_id(msg)
      assert result == :erlang.phash2("unexpected.format")
    end
  end

  describe "pipeline_name/2" do
    test "returns an atom combining namespace and type" do
      assert EventProcessor.pipeline_name("myns", "mytype") == :broadway_myns_mytype
    end
  end
end
