defmodule QuantaCore.MixProject do
  use Mix.Project

  def project do
    [
      app: :quanta_core,
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
      extra_applications: [:logger]
    ]
  end

  defp deps do
    [
      {:jason, "~> 1.4"},
      {:yaml_elixir, "~> 2.12"},
      {:propcheck, "~> 1.4", only: :test}
    ]
  end
end
