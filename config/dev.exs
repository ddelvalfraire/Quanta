import Config

config :logger, level: :debug

config :libcluster,
  topologies: [
    gossip: [strategy: Cluster.Strategy.Gossip]
  ]
