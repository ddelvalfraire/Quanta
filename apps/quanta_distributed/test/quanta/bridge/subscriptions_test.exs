defmodule Quanta.Bridge.SubscriptionsTest do
  use ExUnit.Case

  alias Quanta.Bridge.{Subjects, Subscriptions}
  alias Quanta.Codec.Bridge, as: BridgeCodec
  alias Quanta.Nats.Core

  @describetag :nats

  defp unique_ns, do: "test-#{System.unique_integer([:positive])}"

  defp build_frame(msg_type \\ :heartbeat) do
    header = %{msg_type: msg_type, sequence: 1, timestamp: 1_000, correlation_id: nil}
    {:ok, frame} = BridgeCodec.encode(header, "test-payload")
    frame
  end

  describe "subscribe_island/unsubscribe_island" do
    test "subscribe_island adds per-island subscription" do
      assert :ok = Subscriptions.subscribe_island("test-island-1")
      assert :ok = Subscriptions.unsubscribe_island("test-island-1")
    end

    test "duplicate subscribe_island is idempotent" do
      assert :ok = Subscriptions.subscribe_island("test-island-2")
      assert :ok = Subscriptions.subscribe_island("test-island-2")
      assert :ok = Subscriptions.unsubscribe_island("test-island-2")
    end

    test "unsubscribe_island for unknown island is ok" do
      assert :ok = Subscriptions.unsubscribe_island("nonexistent-island")
    end
  end

  describe "message dispatch" do
    test "receives and decodes d2r messages via catch-all" do
      ns = Application.get_env(:quanta_distributed, :bridge_namespace, "default")
      subject = Subjects.d2r(ns, "island-99", "player", "p1")
      frame = build_frame(:player_join)

      Process.sleep(50)
      Core.publish(subject, frame)

      # The GenServer handles the message internally (logs it).
      # We verify no crash by checking the process is still alive.
      Process.sleep(100)
      assert Process.whereis(Subscriptions) != nil
    end
  end
end
