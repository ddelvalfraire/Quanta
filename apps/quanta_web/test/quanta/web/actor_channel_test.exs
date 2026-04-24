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

  # HIGH-5: handle_in("input", %{"input_seq" => input_seq, ...}, socket)
  # has a guard `when is_integer(input_seq) and input_seq >= 0`
  # (actor_channel.ex:35-36).  When a client sends a non-integer input_seq
  # (e.g. "foo"), a negative integer, or omits the field, no clause matches
  # and the BEAM raises FunctionClauseError, terminating the channel process.
  #
  # A hardened implementation should return {:reply, {:error, ...}} and keep
  # the channel alive.  Currently the channel process crashes.
  describe "HIGH-5: non-integer input_seq crashes the channel process" do
    test "string input_seq kills the channel — should stay alive" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})
      {:ok, _reply, socket} = subscribe_and_join(socket, "actor:test:counter:input-bad-1", %{})

      channel_pid = socket.channel_pid
      assert Process.alive?(channel_pid), "channel should be alive before push"

      # Sending a string input_seq violates the is_integer/1 guard.
      # With the bug this raises FunctionClauseError and kills the channel.
      push(socket, "input", %{"input_seq" => "foo", "data" => ""})

      # Give the scheduler a moment to deliver the message and any crash.
      Process.sleep(50)

      # Documents the FIXED expected behaviour — FAILS today because
      # FunctionClauseError is unhandled and the channel process is dead.
      assert Process.alive?(channel_pid),
             "channel should survive a string input_seq push " <>
               "(bug: FunctionClauseError terminates the channel)"
    end

    test "missing input_seq field kills the channel — should stay alive" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})
      {:ok, _reply, socket} = subscribe_and_join(socket, "actor:test:counter:input-bad-2", %{})

      channel_pid = socket.channel_pid
      assert Process.alive?(channel_pid), "channel should be alive before push"

      # No input_seq key at all — pattern match fails entirely.
      push(socket, "input", %{"data" => "hello"})

      Process.sleep(50)

      # BUG: channel process is dead because no clause matched.
      assert Process.alive?(channel_pid),
             "channel should survive a push with missing input_seq " <>
               "(bug: FunctionClauseError terminates the channel)"
    end

    test "negative integer input_seq kills the channel — should stay alive" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})
      {:ok, _reply, socket} = subscribe_and_join(socket, "actor:test:counter:input-bad-3", %{})

      channel_pid = socket.channel_pid
      assert Process.alive?(channel_pid), "channel should be alive before push"

      # -1 is an integer but fails the `input_seq >= 0` guard; no fallback clause.
      push(socket, "input", %{"input_seq" => -1, "data" => ""})

      Process.sleep(50)

      # BUG: channel crashes because the guard rejects -1 and no clause handles it.
      assert Process.alive?(channel_pid),
             "channel should survive a negative input_seq push " <>
               "(bug: FunctionClauseError terminates the channel)"
    end
  end
end
