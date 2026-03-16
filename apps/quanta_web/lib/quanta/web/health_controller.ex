defmodule Quanta.Web.HealthController do
  use Phoenix.Controller, formats: [:json]

  def live(conn, _params) do
    json(conn, %{status: "ok"})
  end

  def ready(conn, _params) do
    json(conn, %{status: "ok"})
  end
end
