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
        _ ->
          conn
          |> put_status(401)
          |> Phoenix.Controller.json(%{error: "unauthorized"})
          |> halt()
      end
    else
      # No token configured — allow (e.g., in dev/test or behind network isolation)
      conn
    end
  end
end
