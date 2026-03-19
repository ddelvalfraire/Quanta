defmodule Quanta.Web.Endpoint do
  use Phoenix.Endpoint, otp_app: :quanta_web

  socket "/ws", Quanta.Web.ActorSocket,
    websocket: [
      connect_info: [:peer_data, :x_headers]
    ]

  plug Plug.Parsers,
    parsers: [:urlencoded, :multipart, :json],
    pass: ["*/*"],
    json_decoder: Jason

  plug Quanta.Web.Plugs.RequestId

  plug Quanta.Web.Router
end
