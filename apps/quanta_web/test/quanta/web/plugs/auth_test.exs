defmodule Quanta.Web.Plugs.AuthTest do
  use Quanta.Web.ConnCase, async: false

  alias Quanta.Web.Plugs.{Auth, RequireScope}

  @admin_key "qk_admin_test_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
  @rw_key "qk_rw_test_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
  @ro_key "qk_ro_test_cccccccccccccccccccccccccccccccc"

  describe "Auth plug" do
    test "accepts valid admin key and sets assigns", %{conn: conn} do
      conn =
        conn
        |> put_req_header("authorization", "Bearer #{@admin_key}")
        |> Auth.call(Auth.init([]))

      refute conn.halted
      assert conn.assigns.auth_scope == :admin
      assert conn.assigns.auth_namespace == "test"
    end

    test "accepts valid rw key", %{conn: conn} do
      conn =
        conn
        |> put_req_header("authorization", "Bearer #{@rw_key}")
        |> Auth.call(Auth.init([]))

      refute conn.halted
      assert conn.assigns.auth_scope == :rw
      assert conn.assigns.auth_namespace == "test"
    end

    test "accepts valid ro key", %{conn: conn} do
      conn =
        conn
        |> put_req_header("authorization", "Bearer #{@ro_key}")
        |> Auth.call(Auth.init([]))

      refute conn.halted
      assert conn.assigns.auth_scope == :ro
      assert conn.assigns.auth_namespace == "test"
    end

    test "returns 401 for missing authorization", %{conn: conn} do
      conn = Auth.call(conn, Auth.init([]))

      assert conn.halted
      assert conn.status == 401
    end

    test "returns 401 for invalid key format", %{conn: conn} do
      conn =
        conn
        |> put_req_header("authorization", "Bearer bad-key-format")
        |> Auth.call(Auth.init([]))

      assert conn.halted
      assert conn.status == 401
    end

    test "returns 401 for wrong key value", %{conn: conn} do
      conn =
        conn
        |> put_req_header("authorization", "Bearer qk_admin_test_zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz")
        |> Auth.call(Auth.init([]))

      assert conn.halted
      assert conn.status == 401
    end
  end

  describe "RequireScope plug" do
    test "ro scope passes ro check", %{conn: conn} do
      conn =
        conn
        |> assign(:auth_scope, :ro)
        |> RequireScope.call(RequireScope.init(:ro))

      refute conn.halted
    end

    test "ro scope fails rw check", %{conn: conn} do
      conn =
        conn
        |> assign(:auth_scope, :ro)
        |> RequireScope.call(RequireScope.init(:rw))

      assert conn.halted
      assert conn.status == 403
    end

    test "rw scope passes ro check", %{conn: conn} do
      conn =
        conn
        |> assign(:auth_scope, :rw)
        |> RequireScope.call(RequireScope.init(:ro))

      refute conn.halted
    end

    test "rw scope fails admin check", %{conn: conn} do
      conn =
        conn
        |> assign(:auth_scope, :rw)
        |> RequireScope.call(RequireScope.init(:admin))

      assert conn.halted
      assert conn.status == 403
    end

    test "admin scope passes all checks", %{conn: conn} do
      for scope <- [:ro, :rw, :admin] do
        conn =
          conn
          |> assign(:auth_scope, :admin)
          |> RequireScope.call(RequireScope.init(scope))

        refute conn.halted
      end
    end
  end

  describe "namespace enforcement" do
    test "rejects request to wrong namespace", %{conn: conn} do
      conn =
        conn
        |> auth(@rw_key)
        |> get("/api/v1/types/other")

      assert conn.status == 403
      assert json_response(conn, 403)["error"] == "namespace_forbidden"
    end

    test "allows request to matching namespace", %{conn: conn} do
      conn =
        conn
        |> auth(@ro_key)
        |> get("/api/v1/types/test")

      assert conn.status == 200
    end
  end

  defp auth(conn, key) do
    put_req_header(conn, "authorization", "Bearer #{key}")
  end
end
