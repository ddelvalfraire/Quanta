defmodule Quanta.Web.ActorChannelTest do
  use Quanta.Web.ChannelCase, async: false

  alias Quanta.Actor.Server

  describe "socket connect" do
    test "connects with valid rw token" do
      assert {:ok, socket} =
               connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      assert socket.assigns.auth_scope == :rw
      assert socket.assigns.auth_namespace == "test"
    end

    test "connects with valid ro token" do
      assert {:ok, socket} =
               connect(Quanta.Web.ActorSocket, %{"token" => @ro_key})

      assert socket.assigns.auth_scope == :ro
    end

    test "rejects invalid token" do
      assert :error = connect(Quanta.Web.ActorSocket, %{"token" => "bad"})
    end

    test "rejects missing token" do
      assert :error = connect(Quanta.Web.ActorSocket, %{})
    end
  end

  describe "join" do
    test "returns base64 state on successful join" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      assert {:ok, reply, _socket} =
               subscribe_and_join(socket, "actor:test:counter:join-1", %{})

      assert %{state: state_b64} = reply
      assert {:ok, <<0::64>>} = Base.decode64(state_b64)
    end

    test "rejects namespace mismatch" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      assert {:error, %{reason: "namespace_forbidden"}} =
               subscribe_and_join(socket, "actor:other:counter:x", %{})
    end

    test "rejects invalid topic format" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      assert {:error, %{reason: "invalid_topic"}} =
               subscribe_and_join(socket, "actor:bad-format", %{})
    end

    test "rejects unknown actor type" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      assert {:error, %{reason: "actor_type_not_found"}} =
               subscribe_and_join(socket, "actor:test:nonexistent:x", %{})
    end
  end

  describe "message" do
    test "send and receive reply" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})
      {:ok, _reply, socket} = subscribe_and_join(socket, "actor:test:counter:msg-1", %{})

      ref = push(socket, "message", %{"payload" => Base.encode64("inc")})
      assert_reply ref, :ok, %{payload: payload_b64}
      assert {:ok, <<1::64>>} = Base.decode64(payload_b64)
    end

    test "no_reply message returns empty map" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})
      {:ok, _reply, socket} = subscribe_and_join(socket, "actor:test:counter:msg-2", %{})

      ref = push(socket, "message", %{"payload" => Base.encode64("no_reply")})
      assert_reply ref, :ok, %{}
    end

    test "ro scope cannot send messages" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @ro_key})
      {:ok, _reply, socket} = subscribe_and_join(socket, "actor:test:counter:msg-3", %{})

      ref = push(socket, "message", %{"payload" => Base.encode64("inc")})
      assert_reply ref, :error, %{reason: "insufficient_scope"}
    end

    test "invalid base64 payload returns error" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})
      {:ok, _reply, socket} = subscribe_and_join(socket, "actor:test:counter:msg-4", %{})

      ref = push(socket, "message", %{"payload" => "not-valid-base64!!!"})
      assert_reply ref, :error, %{reason: "invalid_base64"}
    end
  end

  describe "drain notification" do
    test "node_draining push includes reconnect_ms" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})
      {:ok, _reply, _socket} = subscribe_and_join(socket, "actor:test:counter:drain-1", %{})

      Phoenix.PubSub.local_broadcast(Quanta.Web.PubSub, "system:drain", :node_draining)

      assert_push "node_draining", %{reconnect_ms: 1_000}
    end
  end

  describe "actor death" do
    test "force_passivate pushes actor_stopped" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})
      {:ok, _reply, socket} = subscribe_and_join(socket, "actor:test:counter:death-1", %{})

      pid = socket.assigns.actor_pid
      Server.force_passivate(pid)

      assert_push "actor_stopped", %{}
    end

    test "actor crash pushes actor_stopped" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})
      {:ok, _reply, socket} = subscribe_and_join(socket, "actor:test:counter:death-2", %{})

      pid = socket.assigns.actor_pid
      Process.exit(pid, :kill)

      assert_push "actor_stopped", %{}
    end
  end
end
