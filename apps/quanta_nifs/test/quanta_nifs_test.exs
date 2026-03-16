defmodule QuantaNifsTest do
  use ExUnit.Case

  alias Quanta.Nifs.Native

  test "NIF is loaded" do
    assert Native.ping() == true
  end

  describe "encode_envelope_header/1 + decode_envelope_header/1" do
    test "roundtrips a full header" do
      header = %{
        message_id: "msg-1",
        wall_us: 1_000_000,
        logical: 42,
        correlation_id: "corr-1",
        causation_id: nil,
        sender: :system,
        metadata: %{"key" => "value"}
      }

      assert {:ok, binary} = Native.encode_envelope_header(header)
      assert is_binary(binary)

      assert {:ok, decoded} = Native.decode_envelope_header(binary)
      assert decoded.message_id == "msg-1"
      assert decoded.wall_us == 1_000_000
      assert decoded.logical == 42
      assert decoded.correlation_id == "corr-1"
      assert decoded.causation_id == nil
      assert decoded.sender == :system
      assert decoded.metadata == %{"key" => "value"}
    end

    test "roundtrips actor sender" do
      header = %{
        message_id: "m",
        wall_us: 0,
        logical: 0,
        correlation_id: nil,
        causation_id: nil,
        sender: {:actor, "myapp", "counter", "abc123"},
        metadata: %{}
      }

      assert {:ok, binary} = Native.encode_envelope_header(header)
      assert {:ok, decoded} = Native.decode_envelope_header(binary)
      assert decoded.sender == {:actor, "myapp", "counter", "abc123"}
    end

    test "roundtrips client sender" do
      header = %{
        message_id: "m",
        wall_us: 0,
        logical: 0,
        correlation_id: nil,
        causation_id: nil,
        sender: {:client, "user-42"},
        metadata: %{}
      }

      assert {:ok, binary} = Native.encode_envelope_header(header)
      assert {:ok, decoded} = Native.decode_envelope_header(binary)
      assert decoded.sender == {:client, "user-42"}
    end

    test "roundtrips nil sender" do
      header = %{
        message_id: "m",
        wall_us: 0,
        logical: 0,
        correlation_id: nil,
        causation_id: nil,
        sender: nil,
        metadata: %{}
      }

      assert {:ok, binary} = Native.encode_envelope_header(header)
      assert {:ok, decoded} = Native.decode_envelope_header(binary)
      assert decoded.sender == nil
    end

    test "rejects corrupt binary" do
      assert {:error, msg} = Native.decode_envelope_header(<<1, 2, 3>>)
      assert is_binary(msg)
    end
  end
end
