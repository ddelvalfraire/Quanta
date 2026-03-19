defmodule Quanta.Actor.SubscriberTrackerTest do
  use Quanta.Web.ChannelCase, async: false

  alias Quanta.Actor.SubscriberTracker
  alias Quanta.ActorId

  describe "any_subscribers?/1" do
    test "returns false when no subscribers" do
      actor_id = %ActorId{namespace: "test", type: "counter", id: "sub-empty"}
      refute SubscriberTracker.any_subscribers?(actor_id)
    end

    test "returns true when a channel is joined" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      {:ok, _reply, _socket} =
        subscribe_and_join(socket, "actor:test:counter:sub-joined", %{})

      actor_id = %ActorId{namespace: "test", type: "counter", id: "sub-joined"}
      assert SubscriberTracker.any_subscribers?(actor_id)
    end
  end
end
