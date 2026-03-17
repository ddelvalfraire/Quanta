defmodule Quanta.HLC.ServerTest do
  use ExUnit.Case, async: false

  alias Quanta.HLC
  alias Quanta.HLC.Server

  setup do
    # May already be running under Quanta.Supervisor in umbrella tests
    case Process.whereis(Server) do
      nil -> start_supervised!(Server)
      _pid -> :ok
    end

    :ok
  end

  describe "now/0" do
    test "returns an HLC struct" do
      assert %HLC{} = Server.now()
    end

    test "successive calls are monotonically increasing" do
      a = Server.now()
      b = Server.now()
      assert HLC.compare(a, b) == :lt
    end
  end

  describe "receive_event/1" do
    test "with a future remote timestamp advances wall clock" do
      local = Server.now()
      future_wall = local.wall + 1_000_000
      remote = %HLC{wall: future_wall, logical: 5}

      result = Server.receive_event(remote)

      assert result.wall >= future_wall
    end

    test "with a past remote timestamp keeps local wall, increments logical" do
      _first = Server.now()
      local = Server.now()
      past_remote = %HLC{wall: local.wall - 1_000_000, logical: 0}

      result = Server.receive_event(past_remote)

      assert result.wall >= local.wall
      assert HLC.compare(local, result) == :lt
    end

    test "result is always greater than both local and remote" do
      local = Server.now()
      remote = %HLC{wall: local.wall, logical: local.logical + 10}

      result = Server.receive_event(remote)

      assert HLC.compare(local, result) == :lt
      assert HLC.compare(remote, result) == :lt
    end
  end
end
