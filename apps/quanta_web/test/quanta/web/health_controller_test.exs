defmodule Quanta.Web.HealthControllerTest do
  use Quanta.Web.ConnCase, async: false

  describe "GET /health/live" do
    test "returns 200 ok", %{conn: conn} do
      conn = get(conn, "/health/live")
      assert json_response(conn, 200) == %{"status" => "ok"}
    end
  end

  describe "GET /health/ready" do
    test "returns 200 when all subsystems are up", %{conn: conn} do
      conn = get(conn, "/health/ready")
      assert json_response(conn, 200) == %{"status" => "ok"}
    end

    test "returns 503 when node is draining", %{conn: conn} do
      :persistent_term.put({Quanta.Drain, :draining}, true)

      conn = get(conn, "/health/ready")
      assert json_response(conn, 503) == %{"status" => "draining"}
    after
      try do
        :persistent_term.erase({Quanta.Drain, :draining})
      rescue
        ArgumentError -> :ok
      end
    end

    test "returns 503 when a critical process is down", %{conn: conn} do
      # Suspend supervisor to prevent auto-restart
      :sys.suspend(Quanta.Supervisor)

      pid = Process.whereis(Quanta.Actor.ManifestRegistry)
      ref = Process.monitor(pid)
      Process.exit(pid, :kill)
      assert_receive {:DOWN, ^ref, :process, ^pid, :killed}

      conn = get(conn, "/health/ready")
      body = json_response(conn, 503)
      assert body["status"] == "degraded"
      assert body["checks"]["manifest_registry"] == false
    after
      :sys.resume(Quanta.Supervisor)
      # Wait for supervisor to restart ManifestRegistry
      Process.sleep(100)
    end
  end
end
