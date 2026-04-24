defmodule Quanta.Web.Plugs.InternalAuthTest do
  # FINDING 1 (CRITICAL-2): Unauthenticated drain endpoint
  #
  # When :quanta_web, :internal_auth_token is nil (the default when the env
  # var is unset), InternalAuth.call/2 falls through to the bare `conn` branch
  # at line 22-23 of internal_auth.ex.  Any caller — authenticated or not —
  # gets a 200 from POST /api/internal/drain.
  #
  # These tests MUST FAIL today:
  #   - "rejects unauthenticated POST /api/internal/drain when token is nil"
  #     fails because the response is 200, not 401.
  #
  # Do NOT fix the underlying plug.  These are RED tests that document the bug.

  use Quanta.Web.ConnCase, async: false

  alias Quanta.Web.Plugs.InternalAuth

  @fast_drain_opts [
    complete_in_flight_delay_ms: 10,
    ordered_passivation_delay_ms: 10,
    force_stop_delay_ms: 200
  ]

  setup do
    original_token = Application.get_env(:quanta_web, :internal_auth_token)

    Application.put_env(:quanta_web, :drain_opts, @fast_drain_opts)

    on_exit(fn ->
      if original_token == nil do
        Application.delete_env(:quanta_web, :internal_auth_token)
      else
        Application.put_env(:quanta_web, :internal_auth_token, original_token)
      end

      Application.delete_env(:quanta_web, :drain_opts)

      try do
        :persistent_term.erase({Quanta.Drain, :draining})
      rescue
        ArgumentError -> :ok
      end

      if pid = Process.whereis(Quanta.Drain) do
        try do
          GenServer.stop(pid, :normal, 1_000)
        catch
          _, _ -> :ok
        end
      end
    end)

    :ok
  end

  # ---------------------------------------------------------------------------
  # Unit-level: call the plug directly with nil token
  # ---------------------------------------------------------------------------

  describe "InternalAuth plug — token is nil" do
    test "halts with 401 when no token is configured and no auth header", %{conn: conn} do
      Application.delete_env(:quanta_web, :internal_auth_token)

      conn_out = InternalAuth.call(conn, InternalAuth.init([]))

      # BUG: today the plug returns conn unchanged (halted? false, status nil).
      # Fixed code must halt and set 401.
      assert conn_out.halted,
             "Expected plug to halt when no token is configured, but conn.halted was false"

      assert conn_out.status == 401,
             "Expected status 401 when no token configured, got #{conn_out.status}"
    end
  end

  # ---------------------------------------------------------------------------
  # Integration-level: POST /api/internal/drain with no auth header
  # ---------------------------------------------------------------------------

  describe "POST /api/internal/drain — no token configured" do
    test "rejects unauthenticated request with 401 when :internal_auth_token is nil", %{
      conn: conn
    } do
      Application.delete_env(:quanta_web, :internal_auth_token)

      conn = post(conn, "/api/internal/drain")

      # BUG: today this returns 200 because InternalAuth passes through when
      # :internal_auth_token is nil (internal_auth.ex line 22-23).
      # The fixed code must return 401.
      assert conn.status == 401,
             "Expected 401 for unauthenticated drain when no token configured, got #{conn.status}"
    end

    test "rejects request bearing an arbitrary bearer token when :internal_auth_token is nil", %{
      conn: conn
    } do
      Application.delete_env(:quanta_web, :internal_auth_token)

      conn =
        conn
        |> put_req_header("authorization", "Bearer any-token")
        |> post("/api/internal/drain")

      assert conn.status == 401,
             "Expected 401 even with a bearer token when no server token configured, got #{conn.status}"
    end
  end

  # ---------------------------------------------------------------------------
  # Sanity: correct behaviour when a token IS configured (these pass today)
  # ---------------------------------------------------------------------------

  describe "POST /api/internal/drain — token is configured" do
    setup do
      Application.put_env(:quanta_web, :internal_auth_token, "correct-token")
      :ok
    end

    test "allows request with matching bearer token", %{conn: conn} do
      conn =
        conn
        |> put_req_header("authorization", "Bearer correct-token")
        |> post("/api/internal/drain")

      assert conn.status == 200
    end

    test "rejects request with wrong bearer token", %{conn: conn} do
      conn =
        conn
        |> put_req_header("authorization", "Bearer wrong-token")
        |> post("/api/internal/drain")

      assert conn.status == 401
    end

    test "rejects request with no authorization header", %{conn: conn} do
      conn = post(conn, "/api/internal/drain")
      assert conn.status == 401
    end
  end
end
