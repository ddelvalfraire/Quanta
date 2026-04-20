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

config :quanta_distributed,
  actor_modules: %{
    {"dev", "file"} => Quanta.Web.Actors.FileActor,
    {"dev", "project"} => Quanta.Web.Actors.ProjectActor
  },
  dev_manifests: [
    %{
      version: "1",
      namespace: "dev",
      type: "file",
      state_kind: {:crdt, :text}
    },
    %{
      version: "1",
      namespace: "dev",
      type: "project",
      state_kind: {:crdt, :tree}
    }
  ]
