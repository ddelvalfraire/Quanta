defmodule QuantaBench.MixProject do
  use Mix.Project

  def project do
    [
      app: :quanta_bench,
      version: "0.1.0",
      elixir: "~> 1.17",
      deps: [{:jason, "~> 1.4"}]
    ]
  end
end
