defmodule QuantaDistributed.Application do
  use Application

  @impl true
  def start(_type, _args) do
    # Syn scope + event handler setup lives in `Quanta.SynConfig`, supervised
    # as the first child of `Quanta.Supervisor`. That prevents the Application
    # callback from crashing the node boot when syn raises on scope conflict
    # or is not yet started (MEDIUM-5).
    children = [
      Quanta.Wasm.JsExecutor,
      Quanta.Supervisor
    ]

    opts = [strategy: :one_for_one, name: QuantaDistributed.Supervisor]
    Supervisor.start_link(children, opts)
  end
end
