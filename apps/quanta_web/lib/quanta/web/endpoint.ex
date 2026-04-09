defmodule Quanta.Web.Endpoint do
  use Phoenix.Endpoint, otp_app: :quanta_web

  socket "/ws", Quanta.Web.ActorSocket,
    websocket: [
      connect_info: [:peer_data, :x_headers],
      error_handler: {Quanta.Web.ActorSocket, :handle_error, []}
    ]

  plug Plug.Parsers,
    parsers: [:urlencoded, :multipart, :json],
    pass: ["*/*"],
    json_decoder: Jason

  plug Quanta.Web.Plugs.RequestId

  plug Plug.Static,
    at: "/",
    from: {:quanta_web, "priv/static"},
    gzip: false,
    only: ~w(assets index.html favicon.ico)

  plug Quanta.Web.Router
end
