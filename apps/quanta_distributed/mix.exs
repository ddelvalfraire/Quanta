defmodule QuantaDistributed.MixProject do
  use Mix.Project

  def project do
    [
      app: :quanta_distributed,
      version: "0.1.0",
      build_path: "../../_build",
      config_path: "../../config/config.exs",
      deps_path: "../../deps",
      lockfile: "../../mix.lock",
      elixir: "~> 1.19",
      elixirc_paths: elixirc_paths(Mix.env()),
      start_permanent: Mix.env() == :prod,
      deps: deps()
    ]
  end

  defp elixirc_paths(:test), do: ["lib", "test/support"]
  defp elixirc_paths(_), do: ["lib"]

  def application do
    [
      extra_applications: [:logger],
      mod: {QuantaDistributed.Application, []}
    ]
  end

  defp deps do
    [
      {:quanta_core, in_umbrella: true},
      {:quanta_nifs, in_umbrella: true},
      {:broadway, "~> 1.2"},
      {:gnat, "~> 1.13"},
      {:syn, "~> 3.4"},
      {:telemetry, "~> 1.3"},
      {:libcluster, "~> 3.5"},
      {:ex_hash_ring, "~> 7.0"}
    ]
  end
end
