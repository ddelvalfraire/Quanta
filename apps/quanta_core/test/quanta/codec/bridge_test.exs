defmodule Quanta.Codec.BridgeTest do
  use ExUnit.Case, async: true

  alias Quanta.Codec.Bridge

  @msg_types [
    :activate_island,
    :deactivate_island,
    :player_join,
    :player_leave,
    :entity_command,
    :state_sync,
    :heartbeat,
    :capacity_report
  ]

  defp build_header(overrides \\ %{}) do
    Map.merge(
      %{
        msg_type: :heartbeat,
        sequence: 1,
        timestamp: 1_000_000,
        correlation_id: nil
      },
      overrides
    )
  end

  describe "encode/decode roundtrip" do
    test "all msg_types roundtrip" do
      for msg_type <- @msg_types do
        header = build_header(%{msg_type: msg_type})
        assert {:ok, frame} = Bridge.encode(header, "test")
        assert {:ok, decoded, payload} = Bridge.decode(frame)
        assert decoded.msg_type == msg_type
        assert payload == "test"
      end
    end

    test "with correlation_id" do
      cid = :crypto.strong_rand_bytes(16)
      header = build_header(%{correlation_id: cid})
      assert {:ok, frame} = Bridge.encode(header, "payload")
      assert {:ok, decoded, _} = Bridge.decode(frame)
      assert decoded.correlation_id == cid
    end

    test "without correlation_id" do
      header = build_header(%{correlation_id: nil})
      assert {:ok, frame} = Bridge.encode(header, "")
      assert {:ok, decoded, _} = Bridge.decode(frame)
      assert decoded.correlation_id == nil
    end

    test "empty payload" do
      header = build_header()
      assert {:ok, frame} = Bridge.encode(header, "")
      assert {:ok, _decoded, payload} = Bridge.decode(frame)
      assert payload == ""
    end

    test "large payload" do
      header = build_header()
      big = :crypto.strong_rand_bytes(50_000)
      assert {:ok, frame} = Bridge.encode(header, big)
      assert {:ok, _decoded, payload} = Bridge.decode(frame)
      assert payload == big
    end

    test "preserves sequence and timestamp" do
      header = build_header(%{sequence: 999_999, timestamp: 1_234_567_890})
      assert {:ok, frame} = Bridge.encode(header, "")
      assert {:ok, decoded, _} = Bridge.decode(frame)
      assert decoded.sequence == 999_999
      assert decoded.timestamp == 1_234_567_890
    end
  end

  describe "decode errors" do
    test "truncated frame" do
      assert {:error, _} = Bridge.decode(<<0x01, 0, 0, 0>>)
    end

    test "invalid version" do
      header = build_header()
      {:ok, frame} = Bridge.encode(header, "")
      <<_version::8, rest::binary>> = frame
      bad_frame = <<0xFF, rest::binary>>
      assert {:error, _} = Bridge.decode(bad_frame)
    end

    test "empty binary" do
      assert {:error, _} = Bridge.decode(<<>>)
    end
  end
end
