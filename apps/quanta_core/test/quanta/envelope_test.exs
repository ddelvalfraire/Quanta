defmodule Quanta.EnvelopeTest do
  use ExUnit.Case, async: true

  alias Quanta.{Envelope, ActorId, HLC}

  describe "new/1" do
    test "auto-generates message_id and timestamp" do
      env = Envelope.new(payload: <<1, 2, 3>>)
      assert is_binary(env.message_id)
      assert String.length(env.message_id) == 26
      assert %HLC{} = env.timestamp
      assert env.timestamp.logical == 0
    end

    test "requires payload" do
      assert_raise ArgumentError, fn -> Envelope.new([]) end
    end

    test "defaults metadata to empty map" do
      env = Envelope.new(payload: "hello")
      assert env.metadata == %{}
    end

    test "defaults optional fields to nil" do
      env = Envelope.new(payload: "hello")
      assert is_nil(env.correlation_id)
      assert is_nil(env.causation_id)
      assert is_nil(env.sender)
    end

    test "accepts all optional fields" do
      sender = %ActorId{namespace: "ns", type: "t", id: "1"}

      env =
        Envelope.new(
          payload: "hello",
          correlation_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
          causation_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
          sender: sender,
          metadata: %{"key" => "val"}
        )

      assert env.sender == sender
      assert env.correlation_id == "01ARZ3NDEKTSV4RRFFQ69G5FAV"
      assert env.metadata == %{"key" => "val"}
    end

    test "allows caller to override message_id and timestamp" do
      hlc = %HLC{wall: 42, logical: 1}
      env = Envelope.new(payload: "x", message_id: "CUSTOM", timestamp: hlc)
      assert env.message_id == "CUSTOM"
      assert env.timestamp == hlc
    end
  end
end
