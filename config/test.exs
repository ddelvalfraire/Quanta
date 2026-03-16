import Config

config :logger, level: :warning

config :quanta_web, Quanta.Web.Endpoint,
  secret_key_base: String.duplicate("a", 64)

config :quanta_nifs,
  wasm_hmac_key: Base.decode16!("DEADBEEFDEADBEEFDEADBEEFDEADBEEFDEADBEEFDEADBEEFDEADBEEFDEADBEEF")
