defmodule QuantaDistributed.Application do
  use Application

  @impl true
  def start(_type, _args) do
    # Must be initialised before syn, so the event handler's callbacks into
    # the local mirror are guaranteed to find the table already there.
    Quanta.Actor.Registry.init_local_index()

    :syn.add_node_to_scopes(Quanta.Actor.Registry.scopes())
    :syn.set_event_handler(Quanta.Actor.SynEventHandler)

    children = [
      Quanta.Wasm.JsExecutor,
      Quanta.Supervisor
    ]

    opts = [strategy: :one_for_one, name: QuantaDistributed.Supervisor]
    Supervisor.start_link(children, opts)
  end
end
