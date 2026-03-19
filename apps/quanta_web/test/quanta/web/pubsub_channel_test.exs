defmodule Quanta.Web.PubSubChannelTest do
  use Quanta.Web.ChannelCase, async: false

  describe "join" do
    test "valid namespace" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      assert {:ok, _reply, _socket} =
               subscribe_and_join(socket, "pubsub:test", %{})
    end

    test "rejects namespace mismatch" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      assert {:error, %{reason: "namespace_forbidden"}} =
               subscribe_and_join(socket, "pubsub:other", %{})
    end
  end

  describe "publish" do
    test "relays messages to other subscribers" do
      {:ok, socket1} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})
      {:ok, _reply, socket1} = subscribe_and_join(socket1, "pubsub:test", %{})

      {:ok, socket2} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})
      {:ok, _reply, _socket2} = subscribe_and_join(socket2, "pubsub:test", %{})

      push(socket1, "publish", %{"data" => "hello"})

      assert_push "publish", %{"data" => "hello"}
    end
  end
end
