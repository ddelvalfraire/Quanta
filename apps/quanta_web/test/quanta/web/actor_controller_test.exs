defmodule Quanta.Web.ActorControllerTest do
  use Quanta.Web.ConnCase, async: false

  alias Quanta.Actor.{DynSup, Server}
  alias Quanta.ActorId

  defp auth(conn, key \\ @rw_key) do
    put_req_header(conn, "authorization", "Bearer #{key}")
  end

  defp start_actor(id, module \\ Quanta.Web.Test.Counter) do
    actor_id = %ActorId{namespace: "test", type: "counter", id: id}
    opts = [actor_id: actor_id, module: module]
    DynSup.start_actor(actor_id, child_spec: {Server, opts})
  end

  describe "POST /api/v1/actors/:ns/:type/:id/messages" do
    test "routes message and returns binary reply", %{conn: conn} do
      {:ok, _} = start_actor("msg-1")

      conn =
        conn
        |> auth()
        |> put_req_header("content-type", "application/octet-stream")
        |> post("/api/v1/actors/test/counter/msg-1/messages", "inc")

      assert conn.status == 200
      assert conn.resp_body == <<1::64>>
      assert get_resp_header(conn, "content-type") |> hd() =~ "application/octet-stream"
    end

    test "returns 202 for no-reply message", %{conn: conn} do
      {:ok, _} = start_actor("msg-nr")

      conn =
        conn
        |> auth()
        |> put_req_header("content-type", "application/octet-stream")
        |> post("/api/v1/actors/test/counter/msg-nr/messages", "no_reply")

      assert conn.status == 202
    end

    test "activates actor on demand", %{conn: conn} do
      conn =
        conn
        |> auth()
        |> put_req_header("content-type", "application/octet-stream")
        |> post("/api/v1/actors/test/counter/msg-new/messages", "inc")

      assert conn.status == 200
      assert conn.resp_body == <<1::64>>
    end

    test "returns 404 for unknown type", %{conn: conn} do
      conn =
        conn
        |> auth()
        |> put_req_header("content-type", "application/octet-stream")
        |> post("/api/v1/actors/test/unknown/x/messages", "inc")

      assert json_response(conn, 404)["error"] == "actor type not found"
    end

    test "returns 400 for invalid actor id", %{conn: conn} do
      conn =
        conn
        |> auth()
        |> put_req_header("content-type", "application/octet-stream")
        |> post("/api/v1/actors/test/counter/bad id!/messages", "inc")

      assert json_response(conn, 400)["error"] == "invalid actor id"
    end

    test "passes X-Quanta-Correlation-Id header", %{conn: conn} do
      {:ok, _} = start_actor("msg-corr")

      conn =
        conn
        |> auth()
        |> put_req_header("content-type", "application/octet-stream")
        |> put_req_header("x-quanta-correlation-id", "test-corr-123")
        |> post("/api/v1/actors/test/counter/msg-corr/messages", "inc")

      assert conn.status == 200
    end

    test "returns request_id in error responses", %{conn: conn} do
      conn =
        conn
        |> auth()
        |> put_req_header("content-type", "application/octet-stream")
        |> post("/api/v1/actors/test/unknown/x/messages", "inc")

      body = json_response(conn, 404)
      assert is_binary(body["request_id"])
      assert body["trace_id"] == nil
    end

    test "requires rw scope", %{conn: conn} do
      conn =
        conn
        |> auth(@ro_key)
        |> put_req_header("content-type", "application/octet-stream")
        |> post("/api/v1/actors/test/counter/x/messages", "inc")

      assert conn.status == 403
    end
  end

  describe "GET /api/v1/actors/:ns/:type/:id/state" do
    test "returns actor state as octet-stream", %{conn: conn} do
      {:ok, _} = start_actor("st-1")

      conn =
        conn
        |> auth(@ro_key)
        |> get("/api/v1/actors/test/counter/st-1/state")

      assert conn.status == 200
      assert conn.resp_body == <<0::64>>
      assert get_resp_header(conn, "content-type") |> hd() =~ "application/octet-stream"
    end

    test "returns 404 for non-existent actor", %{conn: conn} do
      conn =
        conn
        |> auth(@ro_key)
        |> get("/api/v1/actors/test/counter/nope/state")

      assert json_response(conn, 404)["error"] == "actor not found"
    end
  end

  describe "GET /api/v1/actors/:ns/:type/:id/meta" do
    test "returns actor metadata as JSON", %{conn: conn} do
      {:ok, _} = start_actor("meta-1")

      conn =
        conn
        |> auth(@ro_key)
        |> get("/api/v1/actors/test/counter/meta-1/meta")

      body = json_response(conn, 200)
      assert body["actor_id"]["namespace"] == "test"
      assert body["actor_id"]["type"] == "counter"
      assert body["actor_id"]["id"] == "meta-1"
      assert body["status"] == "active"
      assert body["message_count"] == 0
      assert is_binary(body["activated_at"])
    end

    test "returns 404 for non-existent actor", %{conn: conn} do
      conn =
        conn
        |> auth(@ro_key)
        |> get("/api/v1/actors/test/counter/nope/meta")

      assert json_response(conn, 404)["error"] == "actor not found"
    end
  end

  describe "POST /api/v1/actors/:ns/:type (spawn)" do
    test "spawns a new actor with generated id", %{conn: conn} do
      conn =
        conn
        |> auth()
        |> put_req_header("content-type", "application/json")
        |> post("/api/v1/actors/test/counter", Jason.encode!(%{}))

      body = json_response(conn, 201)
      assert body["actor_id"]["namespace"] == "test"
      assert body["actor_id"]["type"] == "counter"
      assert is_binary(body["actor_id"]["id"])
    end

    test "spawns with explicit id", %{conn: conn} do
      conn =
        conn
        |> auth()
        |> put_req_header("content-type", "application/json")
        |> post("/api/v1/actors/test/counter", Jason.encode!(%{id: "explicit-1"}))

      body = json_response(conn, 201)
      assert body["actor_id"]["id"] == "explicit-1"
    end

    test "returns 409 for duplicate spawn", %{conn: conn} do
      {:ok, _} = start_actor("dup-1")

      conn =
        conn
        |> auth()
        |> put_req_header("content-type", "application/json")
        |> post("/api/v1/actors/test/counter", Jason.encode!(%{id: "dup-1"}))

      assert json_response(conn, 409)["error"] == "actor already exists"
    end

    test "returns 404 for unknown type", %{conn: conn} do
      conn =
        conn
        |> auth()
        |> put_req_header("content-type", "application/json")
        |> post("/api/v1/actors/test/unknown", Jason.encode!(%{}))

      assert json_response(conn, 404)["error"] == "actor type not found"
    end

    test "requires rw scope", %{conn: conn} do
      conn =
        conn
        |> auth(@ro_key)
        |> put_req_header("content-type", "application/json")
        |> post("/api/v1/actors/test/counter", Jason.encode!(%{}))

      assert conn.status == 403
    end
  end

  describe "DELETE /api/v1/actors/:ns/:type/:id (destroy)" do
    test "destroys an active actor", %{conn: conn} do
      {:ok, _} = start_actor("del-1")

      conn =
        conn
        |> auth()
        |> delete("/api/v1/actors/test/counter/del-1")

      assert conn.status == 204
    end

    test "returns 404 for non-existent actor", %{conn: conn} do
      conn =
        conn
        |> auth()
        |> delete("/api/v1/actors/test/counter/nope")

      assert json_response(conn, 404)["error"] == "actor not found"
    end
  end
end
