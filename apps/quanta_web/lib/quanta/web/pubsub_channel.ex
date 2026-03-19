defmodule Quanta.Web.PubSubChannel do
  use Phoenix.Channel

  @impl true
  def join("pubsub:" <> namespace, _params, socket) do
    if namespace == socket.assigns.auth_namespace do
      {:ok, socket}
    else
      {:error, %{reason: "namespace_forbidden"}}
    end
  end

  @impl true
  def handle_in("publish", payload, socket) do
    if socket.assigns.auth_scope == :ro do
      {:reply, {:error, %{reason: "insufficient_scope"}}, socket}
    else
      broadcast_from!(socket, "publish", payload)
      {:noreply, socket}
    end
  end
end
