defmodule QuantaWeb.Application do
  use Application

  @impl true
  def start(_type, _args) do
    children = [
      {Phoenix.PubSub, name: Quanta.Web.PubSub},
      Quanta.Web.Presence,
      {Quanta.Web.Endpoint, []}
    ]

    opts = [strategy: :one_for_one, name: QuantaWeb.Supervisor]
    Supervisor.start_link(children, opts)
  end
end
