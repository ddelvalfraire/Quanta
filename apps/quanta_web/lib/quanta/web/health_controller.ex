defmodule Quanta.Web.HealthController do
  use Phoenix.Controller, formats: [:json]

  def live(conn, _params) do
    json(conn, %{status: "ok"})
  end

  def ready(conn, _params) do
    checks = %{
      supervisor: process_alive?(Quanta.Supervisor),
      manifest_registry: process_alive?(Quanta.Actor.ManifestRegistry),
      dyn_sup: process_alive?(Quanta.Actor.DynSup)
    }

    if Enum.all?(checks, fn {_k, v} -> v end) do
      json(conn, %{status: "ok"})
    else
      conn
      |> put_status(503)
      |> json(%{status: "degraded", checks: checks})
    end
  end

  defp process_alive?(name) do
    case Process.whereis(name) do
      nil -> false
      pid -> Process.alive?(pid)
    end
  end
end
