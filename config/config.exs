import Config

config :quanta_web, Quanta.Web.Endpoint,
  url: [host: "localhost"],
  render_errors: [formats: [json: Quanta.Web.ErrorJSON]],
  pubsub_server: Quanta.Web.PubSub,
  server: false

config :phoenix, :json_library, Jason

config :logger, :default_formatter, format: "$time $metadata[$level] $message\n"

import_config "#{config_env()}.exs"
