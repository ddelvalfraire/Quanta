defmodule Quanta.Codec.SnapshotTest do
  use ExUnit.Case, async: true

  alias Quanta.Codec.Snapshot

  describe "encode/4 + decode/1 roundtrip" do
    test "basic roundtrip" do
      encoded = Snapshot.encode(42, 1, 99, "hello")
      assert {:ok, header, "hello"} = Snapshot.decode(encoded)
      assert header.js_seq == 42
      assert header.state_version == 1
      assert header.activation_nonce == 99
    end

    test "empty state_data" do
      encoded = Snapshot.encode(1, 1, 1, "")
      assert {:ok, _header, ""} = Snapshot.decode(encoded)
    end

    test "large state_data" do
      data = :crypto.strong_rand_bytes(100_000)
      encoded = Snapshot.encode(1, 1, 1, data)
      assert {:ok, _header, ^data} = Snapshot.decode(encoded)
    end

    test "max u64 for js_seq" do
      max_u64 = 0xFFFFFFFFFFFFFFFF
      encoded = Snapshot.encode(max_u64, 1, 0, "x")
      assert {:ok, header, "x"} = Snapshot.decode(encoded)
      assert header.js_seq == max_u64
    end

    test "max u16 for state_version" do
      encoded = Snapshot.encode(0, 65535, 0, "x")
      assert {:ok, header, "x"} = Snapshot.decode(encoded)
      assert header.state_version == 65535
    end

    test "max u64 for activation_nonce" do
      max_u64 = 0xFFFFFFFFFFFFFFFF
      encoded = Snapshot.encode(0, 1, max_u64, "x")
      assert {:ok, header, "x"} = Snapshot.decode(encoded)
      assert header.activation_nonce == max_u64
    end

    test "zero values" do
      encoded = Snapshot.encode(0, 0, 0, "")
      assert {:ok, header, ""} = Snapshot.decode(encoded)
      assert header.js_seq == 0
      assert header.state_version == 0
      assert header.activation_nonce == 0
    end
  end

  describe "header_size/0" do
    test "returns 18" do
      assert Snapshot.header_size() == 18
    end

    test "encoded header is exactly 18 bytes before state_data" do
      encoded = Snapshot.encode(1, 2, 3, "")
      assert byte_size(encoded) == 18
    end
  end

  describe "cross-language golden bytes" do
    test "encode produces known byte sequence (must match Rust)" do
      # This exact byte sequence is also asserted in rust/quanta-core-rs tests.
      # If either side changes encoding, this test catches the drift.
      expected =
        <<0, 0, 0, 0, 0, 0, 0, 42, 0, 1, 0, 0, 0, 0, 0, 0, 0, 99>> <> "hello"

      assert Snapshot.encode(42, 1, 99, "hello") == expected
    end

    test "decode known byte sequence from Rust" do
      bytes = <<0, 0, 0, 0, 0, 0, 0, 42, 0, 1, 0, 0, 0, 0, 0, 0, 0, 99>> <> "hello"
      assert {:ok, header, "hello"} = Snapshot.decode(bytes)
      assert header.js_seq == 42
      assert header.state_version == 1
      assert header.activation_nonce == 99
    end
  end

  describe "decode/1 errors" do
    test "rejects truncated input (< 18 bytes)" do
      assert {:error, :invalid_snapshot} = Snapshot.decode(<<0::8*17>>)
    end

    test "rejects empty binary" do
      assert {:error, :invalid_snapshot} = Snapshot.decode(<<>>)
    end
  end
end
