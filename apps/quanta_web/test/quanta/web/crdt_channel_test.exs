defmodule Quanta.Web.CrdtChannelTest do
  use Quanta.Web.ChannelCase, async: false

  alias Quanta.Actor.Server
  alias Quanta.Nifs.LoroEngine

  describe "join" do
    test "returns base64 Loro snapshot" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      assert {:ok, reply, _socket} =
               subscribe_and_join(socket, "crdt:test:crdt_doc:join-1", %{})

      assert %{snapshot: snapshot_b64} = reply
      assert {:ok, snapshot} = Base.decode64(snapshot_b64)
      assert is_binary(snapshot)
      assert byte_size(snapshot) > 0
    end

    test "rejects namespace mismatch" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      assert {:error, %{reason: "namespace_forbidden"}} =
               subscribe_and_join(socket, "crdt:other:crdt_doc:x", %{})
    end

    test "rejects invalid topic format" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      assert {:error, %{reason: "invalid_topic"}} =
               subscribe_and_join(socket, "crdt:bad-format", %{})
    end
  end

  describe "crdt_update" do
    test "client delta is cast to actor" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})
      {:ok, _reply, socket} = subscribe_and_join(socket, "crdt:test:crdt_doc:update-1", %{})

      {:ok, doc} = LoroEngine.doc_new()
      :ok = LoroEngine.map_set(doc, "root", "extra", "data")
      {:ok, snapshot} = LoroEngine.doc_export_snapshot(doc)

      ref = push(socket, "crdt_update", %{"delta" => Base.encode64(snapshot)})
      assert_reply ref, :ok, %{}
    end

    test "ro scope rejected" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @ro_key})
      {:ok, _reply, socket} = subscribe_and_join(socket, "crdt:test:crdt_doc:update-ro", %{})

      ref = push(socket, "crdt_update", %{"delta" => Base.encode64("data")})
      assert_reply ref, :error, %{reason: "insufficient_scope"}
    end

    test "invalid base64 rejected" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})
      {:ok, _reply, socket} = subscribe_and_join(socket, "crdt:test:crdt_doc:update-bad", %{})

      ref = push(socket, "crdt_update", %{"delta" => "not-valid-base64!!!"})
      assert_reply ref, :error, %{reason: "invalid_base64"}
    end

    test "delta too large rejected" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})
      {:ok, _reply, socket} = subscribe_and_join(socket, "crdt:test:crdt_doc:update-big", %{})

      big_delta = :crypto.strong_rand_bytes(1_048_577)
      ref = push(socket, "crdt_update", %{"delta" => Base.encode64(big_delta)})
      assert_reply ref, :error, %{reason: "delta_too_large"}
    end
  end

  describe "message" do
    test "routed via send_message, reply returned" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})
      {:ok, _reply, socket} = subscribe_and_join(socket, "crdt:test:crdt_doc:msg-1", %{})

      ref = push(socket, "message", %{"payload" => Base.encode64("cmd:hello")})
      assert_reply ref, :ok, %{payload: payload_b64}
      assert {:ok, "ack:hello"} = Base.decode64(payload_b64)
    end
  end

  describe "delta broadcast" do
    test "pushed to channel, not echoed to sender" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      {:ok, _reply, socket} =
        subscribe_and_join(socket, "crdt:test:crdt_doc:bcast-1", %{"user_id" => "alice"})

      pid = socket.assigns.actor_pid

      {:ok, doc} = LoroEngine.doc_new()
      :ok = LoroEngine.map_set(doc, "root", "key", "val")
      {:ok, snapshot} = LoroEngine.doc_export_snapshot(doc)

      GenServer.cast(pid, {:crdt_delta, snapshot, "bob"})

      assert_push "crdt_update", %{delta: delta_b64, peer_id: "bob"}
      assert {:ok, _} = Base.decode64(delta_b64)
    end

    test "delta from same user_id is not pushed" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      {:ok, _reply, socket} =
        subscribe_and_join(socket, "crdt:test:crdt_doc:bcast-2", %{"user_id" => "alice"})

      pid = socket.assigns.actor_pid

      {:ok, doc} = LoroEngine.doc_new()
      :ok = LoroEngine.map_set(doc, "root", "key", "val")
      {:ok, snapshot} = LoroEngine.doc_export_snapshot(doc)

      GenServer.cast(pid, {:crdt_delta, snapshot, "alice"})

      refute_push "crdt_update", %{}, 200
    end
  end

  describe "command-originated CRDT ops" do
    test "crdt_ops from handle_message are broadcast to subscribers" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      {:ok, _reply, socket} =
        subscribe_and_join(socket, "crdt:test:crdt_doc:cmd-bcast-1", %{"user_id" => "alice"})

      pid = socket.assigns.actor_pid
      envelope = Quanta.Envelope.new(payload: "map_set:key:value", sender: {:client, "channel"})
      Server.send_message(pid, envelope)

      assert_push "crdt_update", %{delta: _delta_b64, peer_id: nil}
    end
  end

  describe "multi-client concurrent sync" do
    test "two clients on the same actor receive each other's deltas" do
      topic = "crdt:test:crdt_doc:multi-1"

      {:ok, sock_a} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})
      {:ok, _reply, sock_a} = subscribe_and_join(sock_a, topic, %{"user_id" => "alice"})

      {:ok, sock_b} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})
      {:ok, _reply, sock_b} = subscribe_and_join(sock_b, topic, %{"user_id" => "bob"})

      {:ok, doc} = LoroEngine.doc_new()
      :ok = LoroEngine.map_set(doc, "root", "from_alice", "hi")
      {:ok, snapshot} = LoroEngine.doc_export_snapshot(doc)

      ref = push(sock_a, "crdt_update", %{"delta" => Base.encode64(snapshot)})
      assert_reply ref, :ok, %{}

      assert_push "crdt_update", %{delta: _, peer_id: "alice"}
    end

    test "sender does not receive own delta echo" do
      topic = "crdt:test:crdt_doc:multi-2"

      {:ok, sock_a} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})
      {:ok, _reply, sock_a} = subscribe_and_join(sock_a, topic, %{"user_id" => "alice"})

      {:ok, sock_b} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})
      {:ok, _reply, _sock_b} = subscribe_and_join(sock_b, topic, %{"user_id" => "bob"})

      {:ok, doc} = LoroEngine.doc_new()
      :ok = LoroEngine.map_set(doc, "root", "key", "val")
      {:ok, snapshot} = LoroEngine.doc_export_snapshot(doc)

      ref = push(sock_a, "crdt_update", %{"delta" => Base.encode64(snapshot)})
      assert_reply ref, :ok, %{}

      assert_push "crdt_update", %{peer_id: "alice"}
      refute_push "crdt_update", %{}, 200
    end

    test "bidirectional: both clients can send and receive" do
      topic = "crdt:test:crdt_doc:multi-3"

      {:ok, sock_a} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})
      {:ok, _reply, sock_a} = subscribe_and_join(sock_a, topic, %{"user_id" => "alice"})

      {:ok, sock_b} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})
      {:ok, _reply, sock_b} = subscribe_and_join(sock_b, topic, %{"user_id" => "bob"})

      {:ok, doc_a} = LoroEngine.doc_new()
      :ok = LoroEngine.map_set(doc_a, "root", "a_key", "a_val")
      {:ok, snap_a} = LoroEngine.doc_export_snapshot(doc_a)

      ref_a = push(sock_a, "crdt_update", %{"delta" => Base.encode64(snap_a)})
      assert_reply ref_a, :ok, %{}
      assert_push "crdt_update", %{peer_id: "alice"}

      {:ok, doc_b} = LoroEngine.doc_new()
      :ok = LoroEngine.map_set(doc_b, "root", "b_key", "b_val")
      {:ok, snap_b} = LoroEngine.doc_export_snapshot(doc_b)

      ref_b = push(sock_b, "crdt_update", %{"delta" => Base.encode64(snap_b)})
      assert_reply ref_b, :ok, %{}
      assert_push "crdt_update", %{peer_id: "bob"}
    end
  end

  describe "drain notification" do
    test "node_draining push includes reconnect_ms" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})
      {:ok, _reply, _socket} = subscribe_and_join(socket, "crdt:test:crdt_doc:drain-1", %{})

      Phoenix.PubSub.local_broadcast(Quanta.Web.PubSub, "system:drain", :node_draining)

      assert_push "node_draining", %{reconnect_ms: 1_000}
    end
  end

  describe "ephemeral_update" do
    test "client sends update, other client receives it" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      {:ok, _reply, socket} =
        subscribe_and_join(socket, "crdt:test:crdt_doc:eph-1", %{"user_id" => "alice"})

      pid = socket.assigns.actor_pid

      GenServer.cast(pid, {:ephemeral_update, "user:bob", "cursor-pos", :other_pid})

      assert_push "ephemeral_update", %{data: data_b64}
      assert {:ok, _} = Base.decode64(data_b64)
    end

    test "sender does not receive own ephemeral echo" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      {:ok, _reply, socket} =
        subscribe_and_join(socket, "crdt:test:crdt_doc:eph-noecho", %{"user_id" => "alice"})

      pid = socket.assigns.actor_pid
      channel_pid = socket.channel_pid

      GenServer.cast(pid, {:ephemeral_update, "user:alice", "cursor", channel_pid})

      refute_push "ephemeral_update", %{}, 200
    end

    test "ro scope silently drops ephemeral updates" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @ro_key})
      {:ok, _reply, socket} = subscribe_and_join(socket, "crdt:test:crdt_doc:eph-ro", %{})

      push(socket, "ephemeral_update", %{"key" => "user:x", "value" => Base.encode64("data")})

      refute_push "ephemeral_update", %{}, 200
    end

    test "invalid base64 is silently dropped" do
      topic = "crdt:test:crdt_doc:eph-bad64"

      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})
      {:ok, _reply, socket} = subscribe_and_join(socket, topic, %{"user_id" => "alice"})

      push(socket, "ephemeral_update", %{"key" => "user:alice", "value" => "not-valid!!!"})

      refute_push "ephemeral_update", %{}, 200
    end

    test "throttle enforced at 10 Hz" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      {:ok, _reply, socket} =
        subscribe_and_join(socket, "crdt:test:crdt_doc:eph-throttle", %{"user_id" => "alice"})

      sync = fn ->
        ref = push(socket, "crdt_update", %{"delta" => "!!!"})
        assert_reply ref, :error, %{reason: "invalid_base64"}
      end

      state0 = :sys.get_state(socket.channel_pid)
      init_at = state0.assigns.last_ephemeral_at

      push(socket, "ephemeral_update", %{"key" => "k1", "value" => Base.encode64("v1")})
      sync.()
      state1 = :sys.get_state(socket.channel_pid)
      last_at1 = state1.assigns.last_ephemeral_at
      assert last_at1 > init_at

      push(socket, "ephemeral_update", %{"key" => "k2", "value" => Base.encode64("v2")})
      sync.()
      state2 = :sys.get_state(socket.channel_pid)
      assert state2.assigns.last_ephemeral_at == last_at1

      Process.sleep(110)
      push(socket, "ephemeral_update", %{"key" => "k3", "value" => Base.encode64("v3")})
      sync.()
      state3 = :sys.get_state(socket.channel_pid)
      assert state3.assigns.last_ephemeral_at > last_at1
    end

    test "initial ephemeral state sent on join" do
      topic = "crdt:test:crdt_doc:eph-init"

      {:ok, sock_a} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})
      {:ok, _reply, sock_a} = subscribe_and_join(sock_a, topic, %{"user_id" => "alice"})

      value = Base.encode64("cursor-data")
      push(sock_a, "ephemeral_update", %{"key" => "user:alice", "value" => value})
      Process.sleep(50)

      {:ok, sock_b} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})
      {:ok, _reply, _sock_b} = subscribe_and_join(sock_b, topic, %{"user_id" => "bob"})

      assert_push "ephemeral_state", %{data: state_b64}
      assert {:ok, _} = Base.decode64(state_b64)
    end
  end

  describe "presence_diff" do
    test "diff with leaves triggers subscriber_left to actor" do
      topic = "crdt:test:crdt_doc:pres-diff"

      {:ok, sock_a} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      {:ok, _reply, sock_a} =
        subscribe_and_join(sock_a, topic, %{"user_id" => "alice"})

      actor_pid = sock_a.assigns.actor_pid

      diff = %{joins: %{}, leaves: %{"departed" => %{metas: []}}}
      send(sock_a.channel_pid, %{event: "presence_diff", payload: diff})
      Process.sleep(50)

      assert Process.alive?(actor_pid)
    end
  end

  describe "actor death" do
    test "force_passivate pushes actor_stopped" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})
      {:ok, _reply, socket} = subscribe_and_join(socket, "crdt:test:crdt_doc:death-1", %{})

      pid = socket.assigns.actor_pid
      Server.force_passivate(pid)

      assert_push "actor_stopped", %{}
    end
  end

end
