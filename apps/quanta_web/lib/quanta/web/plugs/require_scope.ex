defmodule Quanta.Web.Plugs.RequireScope do
  @moduledoc false
  import Quanta.Web.ErrorHelpers, only: [error_response: 2]

  @scope_levels %{admin: 3, rw: 2, ro: 1}

  def init(required_scope) when is_atom(required_scope), do: required_scope

  def call(conn, required_scope) do
    if scope_level(conn.assigns[:auth_scope]) >= scope_level(required_scope) do
      conn
    else
      error_response(conn, :insufficient_scope) |> Plug.Conn.halt()
    end
  end

  defp scope_level(scope), do: Map.get(@scope_levels, scope, 0)
end
