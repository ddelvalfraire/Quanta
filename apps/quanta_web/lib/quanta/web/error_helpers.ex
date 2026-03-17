defmodule Quanta.Web.ErrorHelpers do
  @moduledoc false
  import Plug.Conn
  import Phoenix.Controller, only: [json: 2]

  @error_map %{
    actor_not_found: {404, "actor not found"},
    actor_type_not_found: {404, "actor type not found"},
    actor_timeout: {408, "actor timeout"},
    rate_limited: {429, "rate limited"},
    node_at_capacity: {503, "node at capacity"},
    module_not_configured: {503, "actor module not configured"},
    invalid_actor_id: {400, "invalid actor id"},
    actor_already_exists: {409, "actor already exists"},
    wasm_not_available: {501, "WASM runtime not available"},
    unauthorized: {401, "unauthorized"},
    namespace_forbidden: {403, "namespace forbidden"},
    insufficient_scope: {403, "insufficient scope"}
  }

  def error_response(conn, error_atom) do
    {status, message} = Map.get(@error_map, error_atom, {500, to_string(error_atom)})

    conn
    |> put_status(status)
    |> json(%{
      error: message,
      request_id: conn.assigns[:request_id],
      trace_id: nil
    })
  end
end
