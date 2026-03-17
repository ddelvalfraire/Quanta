defmodule Quanta.Web.Plugs.Auth do
  @moduledoc false
  import Plug.Conn
  import Phoenix.Controller, only: [json: 2]

  @key_pattern ~r/^qk_(admin|rw|ro)_([a-zA-Z0-9_-]+)_([a-zA-Z0-9]{32})$/

  def init(opts), do: opts

  def call(conn, _opts) do
    with ["Bearer " <> token] <- get_req_header(conn, "authorization"),
         {:ok, scope, namespace} <- parse_key(token),
         :ok <- validate_key(token) do
      conn
      |> assign(:auth_scope, scope)
      |> assign(:auth_namespace, namespace)
    else
      _ -> halt_unauthorized(conn)
    end
  end

  defp parse_key(token) do
    case Regex.run(@key_pattern, token) do
      [_, scope_str, namespace, _random] ->
        {:ok, String.to_existing_atom(scope_str), namespace}

      _ ->
        :error
    end
  rescue
    ArgumentError -> :error
  end

  defp validate_key(token) do
    api_keys = Application.get_env(:quanta_web, :api_keys, [])

    if Enum.any?(api_keys, &Plug.Crypto.secure_compare(&1, token)) do
      :ok
    else
      :error
    end
  end

  defp halt_unauthorized(conn) do
    conn
    |> put_status(401)
    |> json(%{error: "unauthorized", request_id: conn.assigns[:request_id], trace_id: nil})
    |> halt()
  end
end

defmodule Quanta.Web.Plugs.RequireScope do
  @moduledoc false
  import Plug.Conn
  import Phoenix.Controller, only: [json: 2]

  @scope_levels %{admin: 3, rw: 2, ro: 1}

  def init(required_scope) when is_atom(required_scope), do: required_scope

  def call(conn, required_scope) do
    user_scope = conn.assigns[:auth_scope]

    if scope_level(user_scope) >= scope_level(required_scope) do
      conn
    else
      conn
      |> put_status(403)
      |> json(%{error: "insufficient scope", request_id: conn.assigns[:request_id], trace_id: nil})
      |> halt()
    end
  end

  defp scope_level(scope), do: Map.get(@scope_levels, scope, 0)
end
