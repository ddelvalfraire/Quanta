defmodule QuantaNifs.MixProject do
  use Mix.Project

  def project do
    [
      app: :quanta_nifs,
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
      {:quanta_core, in_umbrella: true},
      {:rustler, "~> 0.37"},
      {:gnat, "~> 1.13", only: :test},
      {:jason, "~> 1.4", only: :test}
    ]
  end
end
