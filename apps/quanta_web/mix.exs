defmodule QuantaWeb.MixProject do
  use Mix.Project

  def project do
    [
      app: :quanta_web,
      version: "0.1.0",
      build_path: "../../_build",
      config_path: "../../config/config.exs",
      deps_path: "../../deps",
      lockfile: "../../mix.lock",
      elixir: "~> 1.19",
      start_permanent: Mix.env() == :prod,
      deps: deps()
    ]
  end

  def application do
    [
      extra_applications: [:logger],
      mod: {QuantaWeb.Application, []}
    ]
  end

  defp deps do
    [
      {:quanta_distributed, in_umbrella: true},
      {:phoenix, "~> 1.8"},
      {:plug_cowboy, "~> 2.7"},
      {:jason, "~> 1.4"},
      {:telemetry, "~> 1.3"}
    ]
  end
end
