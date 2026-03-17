import Config

# --- Logging ---

valid_log_levels = ~w(debug info notice warning error critical alert emergency)

log_level = System.get_env("QUANTA_LOG_LEVEL", "info")

unless log_level in valid_log_levels do
  raise "Invalid QUANTA_LOG_LEVEL: #{inspect(log_level)}. Must be one of: #{Enum.join(valid_log_levels, ", ")}"
end

config :logger, level: String.to_existing_atom(log_level)

# --- Distributed runtime ---

config :quanta_distributed,
  nats_urls:
    System.get_env("QUANTA_NATS_URLS", "nats://localhost:4222") |> String.split(","),
  nats_pool_size:
    System.get_env("QUANTA_NATS_POOL_SIZE", "2") |> String.to_integer(),
  cluster_name:
    System.get_env("QUANTA_CLUSTER_NAME", "quanta"),
  node_name:
    System.get_env("QUANTA_NODE_NAME", "quanta@#{:inet.gethostname() |> elem(1)}"),
  default_idle_timeout:
    System.get_env("QUANTA_DEFAULT_IDLE_TIMEOUT_MS", "300000") |> String.to_integer(),
  snapshot_interval:
    System.get_env("QUANTA_SNAPSHOT_INTERVAL", "100") |> String.to_integer(),
  max_actors_per_node:
    System.get_env("QUANTA_MAX_ACTORS_PER_NODE", "1000000") |> String.to_integer()

# --- NIFs ---

if config_env() != :test do
  config :quanta_nifs,
    wasm_hmac_key:
      System.fetch_env!("QUANTA_WASM_HMAC_KEY") |> Base.decode16!(case: :mixed),
    default_fuel_limit:
      System.get_env("QUANTA_DEFAULT_FUEL_LIMIT", "1000000") |> String.to_integer(),
    default_memory_limit_bytes:
      (System.get_env("QUANTA_DEFAULT_MEMORY_LIMIT_MB", "16") |> String.to_integer()) *
        1_048_576
end

# --- Web / Phoenix ---

if config_env() != :test do
  config :quanta_web, Quanta.Web.Endpoint,
    secret_key_base: System.fetch_env!("QUANTA_SECRET_KEY_BASE"),
    http: [port: System.get_env("QUANTA_HTTP_PORT", "4000") |> String.to_integer()],
    url: [host: System.get_env("QUANTA_HOST", "localhost")]

  config :quanta_web,
    api_keys:
      System.fetch_env!("QUANTA_API_KEYS") |> String.split(",", trim: true)
end

# --- Telemetry ---

if otel_endpoint = System.get_env("QUANTA_OTEL_ENDPOINT") do
  config :quanta_distributed, otel_endpoint: otel_endpoint
end
