defmodule Quanta.Web.ErrorHelpers do
  @moduledoc false
  import Plug.Conn
  import Phoenix.Controller, only: [json: 2]

  @error_map %{
    actor_not_found: {404, "actor not found"},
    actor_type_not_found: {404, "actor type not found"},
    actor_timeout: {408, "actor timeout"},
    rate_limited: {429, "rate limited"},
    overloaded: {503, "server overloaded"},
    node_at_capacity: {503, "node at capacity"},
    module_not_configured: {503, "actor module not configured"},
    invalid_actor_id: {400, "invalid actor id"},
    actor_already_exists: {409, "actor already exists"},
    payload_too_large: {413, "payload too large"},
    wasm_not_available: {501, "WASM runtime not available"},
    unauthorized: {401, "unauthorized"},
    namespace_forbidden: {403, "namespace forbidden"},
    insufficient_scope: {403, "insufficient scope"}
  }

  def error_response(conn, error_atom) when is_atom(error_atom) do
    {status, message} = Map.get(@error_map, error_atom, {500, "internal error"})

    conn
    |> put_status(status)
    |> json(error_body(to_string(error_atom), message, conn))
  end

  def error_response(conn, status, message) when is_integer(status) and is_binary(message) do
    conn
    |> put_status(status)
    |> json(error_body(message, message, conn))
  end

  def error_response(conn, status, message, details)
      when is_integer(status) and is_binary(message) and is_list(details) do
    conn
    |> put_status(status)
    |> json(Map.put(error_body(message, message, conn), :details, details))
  end

  def error_body(error, message, conn) do
    %{
      error: error,
      message: message,
      request_id: conn.assigns[:request_id],
      trace_id: nil
    }
  end
end
