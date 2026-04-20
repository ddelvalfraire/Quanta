defmodule Quanta.Web.ProjectActorTest do
  use Quanta.Web.ChannelCase, async: false

  describe "join" do
    test "returns snapshot for new project" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      assert {:ok, reply, _socket} =
               subscribe_and_join(socket, "crdt:test:project:proj-1", %{})

      assert %{snapshot: snapshot_b64} = reply
      assert is_binary(snapshot_b64)
    end

    test "presence_state pushed after join" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      {:ok, _reply, _socket} =
        subscribe_and_join(socket, "crdt:test:project:pres-1", %{})

      assert_push "presence_state", %{}
    end
  end

  describe "crdt_update" do
    test "tree delta from one client is received by another" do
      {:ok, sock_a} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      {:ok, _reply, sock_a} =
        subscribe_and_join(sock_a, "crdt:test:project:collab-1", %{})

      {:ok, sock_b} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      {:ok, _reply, _sock_b} =
        subscribe_and_join(sock_b, "crdt:test:project:collab-1", %{"user_id" => "bob"})

      # Create a LoroDoc with a tree operation
      alias Quanta.Nifs.LoroEngine
      {:ok, doc} = LoroEngine.doc_new()
      {:ok, _node_id} = LoroEngine.tree_create_node(doc, "tree")
      {:ok, delta} = LoroEngine.doc_export_snapshot(doc)

      ref = push(sock_a, "crdt_update", %{"delta" => Base.encode64(delta)})
      assert_reply ref, :ok, %{}

      assert_push "crdt_update", %{delta: _, peer_id: _}
    end
  end

  describe "file actor independence" do
    test "file actor still works when project actor is active" do
      {:ok, proj_sock} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      {:ok, _reply, _proj_sock} =
        subscribe_and_join(proj_sock, "crdt:test:project:indep-1", %{})

      {:ok, file_sock} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      {:ok, reply, _file_sock} =
        subscribe_and_join(file_sock, "crdt:test:file:indep-1", %{"user_id" => "bob"})

      assert %{snapshot: _} = reply
    end
  end
end
