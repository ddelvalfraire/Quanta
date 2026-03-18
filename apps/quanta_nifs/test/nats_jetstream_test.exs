defmodule Quanta.Nifs.NatsJetstreamTest do
  use ExUnit.Case, async: false

  @moduletag :nats

  describe "nats_connect/2" do
    test "connects to a local NATS server" do
      assert {:ok, conn} = Quanta.Nifs.Native.nats_connect(["nats://localhost:4222"], %{})
      assert is_reference(conn)
    end

    test "connects with custom options" do
      opts = %{max_in_flight: 100, connect_timeout_ms: 2_000}
      assert {:ok, conn} = Quanta.Nifs.Native.nats_connect(["nats://localhost:4222"], opts)
      assert is_reference(conn)
    end

    test "returns error for unreachable server" do
      assert {:error, reason} =
               Quanta.Nifs.Native.nats_connect(["nats://localhost:19999"], %{connect_timeout_ms: 500})

      assert is_binary(reason)
      assert reason =~ "connect_error"
    end
  end
end
