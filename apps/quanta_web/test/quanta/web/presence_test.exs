defmodule Quanta.Web.PresenceTest do
  use Quanta.Web.ChannelCase, async: false

  alias Quanta.Web.Presence

  describe "presence tracking and subscriber_count/1" do
    test "tracks a user and counts subscribers" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      {:ok, _reply, socket} =
        subscribe_and_join(socket, "actor:test:counter:pres-1", %{})

      topic = "actor:test:counter:pres-1"
      assert Presence.subscriber_count(topic) == 1
      assert socket.assigns.user_id != nil
    end

    test "subscriber_count returns 0 for empty topic" do
      assert Presence.subscriber_count("actor:test:counter:no-one") == 0
    end

    test "accepts optional user_id from join params" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      {:ok, _reply, socket} =
        subscribe_and_join(socket, "actor:test:counter:pres-uid", %{"user_id" => "custom-user"})

      assert socket.assigns.user_id == "custom-user"

      topic = "actor:test:counter:pres-uid"
      presences = Presence.list(topic)
      assert Map.has_key?(presences, "custom-user")
    end

    test "presence_state is pushed after join" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      {:ok, _reply, _socket} =
        subscribe_and_join(socket, "actor:test:counter:pres-state", %{})

      assert_push "presence_state", %{}
    end

    test "rejects invalid user_id and generates random fallback" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      {:ok, _reply, socket} =
        subscribe_and_join(socket, "actor:test:counter:pres-bad-uid", %{"user_id" => "bad user!@#"})

      refute socket.assigns.user_id == "bad user!@#"
      assert is_binary(socket.assigns.user_id)
      assert byte_size(socket.assigns.user_id) > 0
    end

    test "generates unique user_id when none provided" do
      {:ok, s1} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})
      {:ok, _reply, s1} = subscribe_and_join(s1, "actor:test:counter:pres-noid-1", %{})

      {:ok, s2} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})
      {:ok, _reply, s2} = subscribe_and_join(s2, "actor:test:counter:pres-noid-2", %{})

      refute s1.assigns.user_id == s2.assigns.user_id
    end

    test "multiple joins increase subscriber count" do
      topic = "actor:test:counter:pres-multi"

      {:ok, s1} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})
      {:ok, _reply, _s1} = subscribe_and_join(s1, topic, %{"user_id" => "user-a"})

      {:ok, s2} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})
      {:ok, _reply, _s2} = subscribe_and_join(s2, topic, %{"user_id" => "user-b"})

      assert Presence.subscriber_count(topic) == 2
    end
  end
end
