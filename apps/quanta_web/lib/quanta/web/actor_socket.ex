defmodule Quanta.Web.ActorSocket do
  use Phoenix.Socket

  channel "actor:*", Quanta.Web.ActorChannel
  channel "pubsub:*", Quanta.Web.PubSubChannel

  @impl true
  def connect(%{"token" => token}, socket, _connect_info) do
    case Quanta.Web.Auth.authenticate(token) do
      {:ok, scope, namespace} ->
        socket =
          socket
          |> assign(:auth_scope, scope)
          |> assign(:auth_namespace, namespace)

        {:ok, socket}

      :error ->
        :error
    end
  end

  def connect(_params, _socket, _connect_info), do: :error

  @impl true
  def id(socket), do: "actor_socket:#{socket.assigns.auth_namespace}"
end
