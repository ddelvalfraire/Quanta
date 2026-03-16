defmodule Quanta.Codec.WireTest do
  use ExUnit.Case, async: true

  alias Quanta.Codec.Wire
  alias Quanta.{Envelope, HLC, ActorId}

  defp build_envelope(overrides \\ []) do
    defaults = [
      message_id: "01HXYZ0000000000000000TEST",
      timestamp: %HLC{wall: 1_000_000, logical: 42},
      payload: "test payload"
    ]

    struct!(Envelope, Keyword.merge(defaults, overrides))
  end

  describe "encode/1 + decode/1 roundtrip" do
    test "minimal envelope (nil optionals)" do
      env = build_envelope()
      encoded = Wire.encode(env)
      assert {:ok, decoded} = Wire.decode(encoded)
      assert decoded.message_id == env.message_id
      assert decoded.timestamp.wall == 1_000_000
      assert decoded.timestamp.logical == 42
      assert decoded.payload == "test payload"
      assert decoded.correlation_id == nil
      assert decoded.causation_id == nil
      assert decoded.sender == nil
      assert decoded.metadata == %{}
    end

    test "full envelope with all fields" do
      sender = %ActorId{namespace: "myapp", type: "counter", id: "abc123"}

      env =
        build_envelope(
          correlation_id: "corr-1",
          causation_id: "cause-1",
          sender: sender,
          metadata: %{"key" => "value", "foo" => "bar"}
        )

      assert {:ok, decoded} = Wire.decode(Wire.encode(env))
      assert decoded.correlation_id == "corr-1"
      assert decoded.causation_id == "cause-1"
      assert decoded.sender == sender
      assert decoded.metadata == %{"key" => "value", "foo" => "bar"}
    end

    test "sender: {:client, id}" do
      env = build_envelope(sender: {:client, "user-42"})
      assert {:ok, decoded} = Wire.decode(Wire.encode(env))
      assert decoded.sender == {:client, "user-42"}
    end

    test "sender: :system" do
      env = build_envelope(sender: :system)
      assert {:ok, decoded} = Wire.decode(Wire.encode(env))
      assert decoded.sender == :system
    end

    test "empty payload" do
      env = build_envelope(payload: "")
      assert {:ok, decoded} = Wire.decode(Wire.encode(env))
      assert decoded.payload == ""
    end

    test "large payload" do
      big = :crypto.strong_rand_bytes(50_000)
      env = build_envelope(payload: big)
      assert {:ok, decoded} = Wire.decode(Wire.encode(env))
      assert decoded.payload == big
    end

    test "empty metadata" do
      env = build_envelope(metadata: %{})
      assert {:ok, decoded} = Wire.decode(Wire.encode(env))
      assert decoded.metadata == %{}
    end
  end

  describe "decode/1 errors" do
    test "rejects unsupported wire version" do
      assert {:error, :unsupported_wire_version} = Wire.decode(<<0x02, 0::32, "data">>)
    end

    test "rejects truncated frame (too short for header)" do
      assert {:error, :invalid_wire_format} = Wire.decode(<<0x01, 100::32, "short">>)
    end

    test "rejects empty binary" do
      assert {:error, :invalid_wire_format} = Wire.decode(<<>>)
    end

    test "rejects single byte" do
      assert {:error, :invalid_wire_format} = Wire.decode(<<0x01>>)
    end
  end

  describe "wire_version/0" do
    test "returns 0x01" do
      assert Wire.wire_version() == 0x01
    end

    test "encoded frame starts with wire version byte" do
      env = build_envelope()
      <<version::8, _::binary>> = Wire.encode(env)
      assert version == 0x01
    end
  end
end
