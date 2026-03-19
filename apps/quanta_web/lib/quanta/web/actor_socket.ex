defmodule Quanta.Web.ActorSocket do
  use Phoenix.Socket

  channel "actor:*", Quanta.Web.ActorChannel
  channel "crdt:*", Quanta.Web.CrdtChannel
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

  def handle_error(conn, :unauthorized) do
    Plug.Conn.send_resp(conn, 403, "Forbidden")
  end
end
