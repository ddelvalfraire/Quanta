defmodule Quanta.Web.DrainControllerTest do
  use Quanta.Web.ConnCase, async: false

  @fast_drain_opts [
    complete_in_flight_delay_ms: 10,
    ordered_passivation_delay_ms: 10,
    force_stop_delay_ms: 200
  ]

  setup do
    Application.put_env(:quanta_web, :drain_opts, @fast_drain_opts)

    on_exit(fn ->
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

      # Re-add self to topology
      if Process.whereis(Quanta.Cluster.Topology) do
        send(Process.whereis(Quanta.Cluster.Topology), {:nodeup, node(), []})
        Quanta.Cluster.Topology.nodes()
      end
    end)

    :ok
  end

  describe "POST /api/internal/drain" do
    test "returns 200 with status drained on completion", %{conn: conn} do
      conn = post(conn, "/api/internal/drain")
      assert json_response(conn, 200) == %{"status" => "drained"}
    end
  end
end
