defmodule Quanta.Web.FileActorTest do
  use Quanta.Web.ChannelCase, async: false

  alias Quanta.Nifs.LoroEngine

  describe "join" do
    test "returns snapshot for new file" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      assert {:ok, reply, _socket} =
               subscribe_and_join(socket, "crdt:test:file:new-1", %{})

      assert %{snapshot: snapshot_b64} = reply
      assert is_binary(snapshot_b64)
    end

    test "presence_state pushed after join" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      {:ok, _reply, _socket} =
        subscribe_and_join(socket, "crdt:test:file:pres-1", %{})

      assert_push "presence_state", %{}
    end
  end

  describe "crdt_update" do
    test "delta from one client is received by another" do
      {:ok, sock_a} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      {:ok, _reply, sock_a} =
        subscribe_and_join(sock_a, "crdt:test:file:collab-1", %{})

      {:ok, sock_b} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      {:ok, _reply, _sock_b} =
        subscribe_and_join(sock_b, "crdt:test:file:collab-1", %{"user_id" => "bob"})

      {:ok, doc} = LoroEngine.doc_new()
      :ok = LoroEngine.text_insert(doc, "codemirror", 0, "hello")
      {:ok, delta} = LoroEngine.doc_export_snapshot(doc)

      ref = push(sock_a, "crdt_update", %{"delta" => Base.encode64(delta)})
      assert_reply ref, :ok, %{}

      assert_push "crdt_update", %{delta: _, peer_id: _}
    end

    test "ro scope rejected" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @ro_key})

      {:ok, _reply, socket} =
        subscribe_and_join(socket, "crdt:test:file:ro-1", %{})

      ref = push(socket, "crdt_update", %{"delta" => Base.encode64("data")})
      assert_reply ref, :error, %{reason: "insufficient_scope"}
    end
  end

  describe "code execution" do
    @tag timeout: 30_000

    test "run command broadcasts output to subscriber" do
      {:ok, sock_a} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      {:ok, _reply, sock_a} =
        subscribe_and_join(sock_a, "crdt:test:file:exec-1", %{})

      payload = Jason.encode!(%{"type" => "run", "code" => ~s[console.log("hello")]})
      ref = push(sock_a, "message", %{"payload" => Base.encode64(payload)})
      assert_reply ref, :ok, %{}, 10_000

      assert_push "execution_output", %{data: json_data}, 5_000
      data = Jason.decode!(json_data)
      assert data["status"] == "ok"
      assert data["stdout"] =~ "hello"
    end

    test "run command error broadcasts error" do
      {:ok, sock_a} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      {:ok, _reply, sock_a} =
        subscribe_and_join(sock_a, "crdt:test:file:exec-2", %{})

      payload = Jason.encode!(%{"type" => "run", "code" => "while(true){}"})
      ref = push(sock_a, "message", %{"payload" => Base.encode64(payload)})
      assert_reply ref, :ok, %{}, 10_000

      assert_push "execution_output", %{data: json_data}, 5_000
      data = Jason.decode!(json_data)
      assert data["status"] == "error"
    end

    test "ro scope cannot execute" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @ro_key})

      {:ok, _reply, socket} =
        subscribe_and_join(socket, "crdt:test:file:exec-3", %{})

      payload = Jason.encode!(%{"type" => "run", "code" => ~s[console.log("hi")]})
      ref = push(socket, "message", %{"payload" => Base.encode64(payload)})
      assert_reply ref, :error, %{reason: "insufficient_scope"}
    end

    test "both subscribers receive execution output" do
      {:ok, sock_a} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      {:ok, _reply, sock_a} =
        subscribe_and_join(sock_a, "crdt:test:file:exec-4", %{})

      {:ok, sock_b} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      {:ok, _reply, _sock_b} =
        subscribe_and_join(sock_b, "crdt:test:file:exec-4", %{"user_id" => "bob"})

      payload = Jason.encode!(%{"type" => "run", "code" => ~s[console.log("shared")]})
      ref = push(sock_a, "message", %{"payload" => Base.encode64(payload)})
      assert_reply ref, :ok, %{}, 10_000

      assert_push "execution_output", %{data: json_data}, 5_000
      data = Jason.decode!(json_data)
      assert data["stdout"] =~ "shared"
    end
  end
end
