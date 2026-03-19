defmodule Quanta.Web.Plugs.Auth do
  @moduledoc false
  import Plug.Conn
  import Quanta.Web.ErrorHelpers, only: [error_response: 2]

  def init(opts), do: opts

  def call(conn, _opts) do
    with ["Bearer " <> token] <- get_req_header(conn, "authorization"),
         {:ok, scope, namespace} <- Quanta.Web.Auth.authenticate(token) do
      conn
      |> assign(:auth_scope, scope)
      |> assign(:auth_namespace, namespace)
    else
      _ -> error_response(conn, :unauthorized) |> halt()
    end
  end
end
