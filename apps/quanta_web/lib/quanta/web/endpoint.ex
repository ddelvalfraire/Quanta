defmodule Quanta.Web.Endpoint do
  use Phoenix.Endpoint, otp_app: :quanta_web

  plug Plug.Parsers,
    parsers: [:urlencoded, :multipart, :json],
    pass: ["*/*"],
    json_decoder: Jason

  plug Quanta.Web.Plugs.RequestId

  plug Quanta.Web.Router
end
