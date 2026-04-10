defmodule QuantaWeb.Application do
  use Application

  @impl true
  def start(_type, _args) do
    register_dev_manifests()

    children = [
      {Phoenix.PubSub, name: Quanta.Web.PubSub},
      Quanta.Web.Presence,
      {Quanta.Web.Endpoint, []}
    ]

    opts = [strategy: :one_for_one, name: QuantaWeb.Supervisor]
    Supervisor.start_link(children, opts)
  end

  defp register_dev_manifests do
    for attrs <- Application.get_env(:quanta_distributed, :dev_manifests, []) do
      manifest = %Quanta.Manifest{
        version: attrs.version,
        namespace: attrs.namespace,
        type: attrs.type,
        state: %Quanta.Manifest.State{kind: attrs.state_kind}
      }

      Quanta.Actor.ManifestRegistry.put(manifest)
    end
  end
end
