defmodule Quanta.Web.Plugs.RequestId do
  @moduledoc false
  import Plug.Conn

  def init(opts), do: opts

  def call(conn, _opts) do
    request_id = Quanta.ULID.generate()

    conn
    |> assign(:request_id, request_id)
    |> put_resp_header("x-request-id", request_id)
  end
end
