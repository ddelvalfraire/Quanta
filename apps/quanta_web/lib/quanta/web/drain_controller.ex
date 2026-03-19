defmodule Quanta.Web.DrainController do
  use Phoenix.Controller, formats: [:json]

  @drain_await_timeout 95_000

  def drain(conn, _params) do
    broadcast_fn = fn ->
      Phoenix.PubSub.local_broadcast(
        Quanta.Web.PubSub,
        "system:drain",
        :node_draining
      )
    end

    drain_opts =
      Application.get_env(:quanta_web, :drain_opts, [])
      |> Keyword.put(:broadcast_fn, broadcast_fn)

    case Quanta.Drain.start_drain(drain_opts) do
      {:ok, _pid} ->
        await_and_respond(conn)

      {:error, {:already_started, _}} ->
        await_and_respond(conn)
    end
  end

  defp await_and_respond(conn) do
    case Quanta.Drain.await(@drain_await_timeout) do
      :ok ->
        json(conn, %{status: "drained"})

      :timeout ->
        conn
        |> put_status(504)
        |> json(%{status: "timeout"})
    end
  end
end
