defmodule Quanta.Web.Plugs.RequireNamespace do
  @moduledoc false
  import Plug.Conn
  import Quanta.Web.ErrorHelpers, only: [error_response: 2]

  def init(opts), do: opts

  def call(conn, _opts) do
    auth_ns = conn.assigns[:auth_namespace]
    request_ns = conn.path_params["ns"]

    if is_nil(request_ns) or auth_ns == request_ns do
      conn
    else
      error_response(conn, :namespace_forbidden) |> halt()
    end
  end
end
