defmodule QuantaDistributed.Application do
  use Application

  @impl true
  def start(_type, _args) do
    :syn.add_node_to_scopes(Quanta.Actor.Registry.scopes())

    children = [
      Quanta.Supervisor
    ]

    opts = [strategy: :one_for_one, name: QuantaDistributed.Supervisor]
    Supervisor.start_link(children, opts)
  end
end
