defmodule Quanta.Web.Plugs.InternalAuth do
  @moduledoc false
  import Plug.Conn

  def init(opts), do: opts

  def call(conn, _opts) do
    expected = Application.get_env(:quanta_web, :internal_auth_token)

    if expected do
      with ["Bearer " <> token] <- get_req_header(conn, "authorization"),
           true <- Plug.Crypto.secure_compare(token, expected) do
        conn
      else
        _ -> unauthorized(conn)
      end
    else
      # No token configured — fail closed. The drain endpoint (and any other
      # caller of this plug) must never be reachable without an explicit
      # :internal_auth_token, even in dev/test, to prevent unauthenticated
      # actions if the env var is accidentally unset in production.
      unauthorized(conn)
    end
  end

  defp unauthorized(conn) do
    conn
    |> put_status(401)
    |> Phoenix.Controller.json(%{error: "unauthorized"})
    |> halt()
  end
end
