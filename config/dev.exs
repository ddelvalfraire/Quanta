import Config

config :logger, level: :debug

config :libcluster,
  topologies: [
    gossip: [strategy: Cluster.Strategy.Gossip]
  ]

config :quanta_web,
  api_keys: [
    "qk_rw_dev_devdevdevdevdevdevdevdevdevdevde"
  ]

config :quanta_web, Quanta.Web.Endpoint,
  http: [port: 4000],
  server: true,
  check_origin: false
