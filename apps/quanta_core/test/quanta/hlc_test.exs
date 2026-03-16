defmodule Quanta.HLCTest do
  use ExUnit.Case, async: true

  alias Quanta.HLC

  describe "now/0" do
    test "returns wall close to current time with logical 0" do
      hlc = HLC.now()
      now_us = System.os_time(:microsecond)
      assert_in_delta hlc.wall, now_us, 1_000_000
      assert hlc.logical == 0
    end
  end

  describe "compare/2" do
    test "earlier wall is :lt" do
      assert HLC.compare(%HLC{wall: 1000, logical: 0}, %HLC{wall: 2000, logical: 0}) == :lt
    end

    test "later wall is :gt" do
      assert HLC.compare(%HLC{wall: 2000, logical: 0}, %HLC{wall: 1000, logical: 0}) == :gt
    end

    test "same wall, lower logical is :lt" do
      assert HLC.compare(%HLC{wall: 1000, logical: 0}, %HLC{wall: 1000, logical: 1}) == :lt
    end

    test "same wall, higher logical is :gt" do
      assert HLC.compare(%HLC{wall: 1000, logical: 5}, %HLC{wall: 1000, logical: 1}) == :gt
    end

    test "identical timestamps are :eq" do
      assert HLC.compare(%HLC{wall: 1000, logical: 5}, %HLC{wall: 1000, logical: 5}) == :eq
    end

    test "wall takes precedence over logical" do
      assert HLC.compare(%HLC{wall: 2000, logical: 0}, %HLC{wall: 1000, logical: 65535}) == :gt
    end
  end

  describe "encode/1 and decode/1" do
    test "roundtrips correctly" do
      hlc = %HLC{wall: 1_700_000_000_000_000, logical: 42}
      assert hlc == hlc |> HLC.encode() |> HLC.decode()
    end

    test "produces exactly 10 bytes" do
      assert byte_size(HLC.encode(%HLC{wall: 0, logical: 0})) == 10
    end

    test "big-endian wire format" do
      assert <<0::56, 1::8, 0, 1>> == HLC.encode(%HLC{wall: 1, logical: 1})
    end

    test "roundtrips max logical" do
      hlc = %HLC{wall: 0, logical: 65535}
      assert hlc == hlc |> HLC.encode() |> HLC.decode()
    end

    test "roundtrips zeros" do
      hlc = %HLC{wall: 0, logical: 0}
      assert hlc == hlc |> HLC.encode() |> HLC.decode()
    end
  end
end
