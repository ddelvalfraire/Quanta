defmodule Quanta.Bridge.SubjectsTest do
  use ExUnit.Case, async: true

  alias Quanta.Bridge.Subjects

  describe "d2r/4" do
    test "builds correct subject" do
      assert Subjects.d2r("prod", "island-1", "player", "p42") ==
               "quanta.prod.bridge.d2r.island-1.player.p42"
    end
  end

  describe "d2r_wildcard/2" do
    test "builds wildcard for island" do
      assert Subjects.d2r_wildcard("prod", "island-1") ==
               "quanta.prod.bridge.d2r.island-1.>"
    end
  end

  describe "d2r_catch_all/1" do
    test "builds catch-all wildcard" do
      assert Subjects.d2r_catch_all("prod") == "quanta.prod.bridge.d2r.>"
    end
  end

  describe "r2d/3" do
    test "builds correct subject" do
      assert Subjects.r2d("prod", "state_sync", "island-1") ==
               "quanta.prod.bridge.r2d.state_sync.island-1"
    end
  end

  describe "r2d_wildcard/1" do
    test "builds wildcard" do
      assert Subjects.r2d_wildcard("prod") == "quanta.prod.bridge.r2d.>"
    end
  end

  describe "r2d_queue_group/0" do
    test "returns fixed group name" do
      assert Subjects.r2d_queue_group() == "quanta-bridge-r2d"
    end
  end

  describe "parse_d2r/1" do
    test "parses valid subject" do
      assert {:ok, parsed} = Subjects.parse_d2r("quanta.prod.bridge.d2r.island-1.player.p42")
      assert parsed.ns == "prod"
      assert parsed.island_id == "island-1"
      assert parsed.type == "player"
      assert parsed.id == "p42"
    end

    test "rejects invalid subject" do
      assert {:error, _} = Subjects.parse_d2r("quanta.prod.cmd.player.p42")
    end

    test "rejects too few segments" do
      assert {:error, _} = Subjects.parse_d2r("quanta.prod.bridge.d2r")
    end

    test "rejects too many segments" do
      assert {:error, _} = Subjects.parse_d2r("quanta.prod.bridge.d2r.a.b.c.d")
    end
  end
end
