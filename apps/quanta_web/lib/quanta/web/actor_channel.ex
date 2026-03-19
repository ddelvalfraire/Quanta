defmodule Quanta.Web.ActorChannel do
  use Phoenix.Channel

  alias Quanta.Actor.{CommandRouter, Server}
  alias Quanta.{ActorId, Envelope}

  @impl true
  def join("actor:" <> rest, _params, socket) do
    case parse_actor_topic(rest) do
      {:ok, actor_id} ->
        if actor_id.namespace != socket.assigns.auth_namespace do
          {:error, %{reason: "namespace_forbidden"}}
        else
          case CommandRouter.ensure_active(actor_id) do
            {:ok, pid} ->
              ref = Process.monitor(pid)
              {:ok, state_data} = Server.get_state(pid)

              socket =
                socket
                |> assign(:actor_id, actor_id)
                |> assign(:actor_pid, pid)
                |> assign(:actor_ref, ref)

              {:ok, %{state: Base.encode64(state_data)}, socket}

            {:error, reason} ->
              {:error, %{reason: to_string(reason)}}
          end
        end

      :error ->
        {:error, %{reason: "invalid_topic"}}
    end
  end

  @impl true
  def handle_in("message", %{"payload" => payload_b64}, socket) do
    if socket.assigns.auth_scope == :ro do
      {:reply, {:error, %{reason: "insufficient_scope"}}, socket}
    else
      with {:ok, payload} <- Base.decode64(payload_b64) do
        envelope = Envelope.new(payload: payload, sender: {:client, "channel"})

        case Server.send_message(socket.assigns.actor_pid, envelope) do
          {:ok, :no_reply} ->
            {:reply, {:ok, %{}}, socket}

          {:ok, reply_data} ->
            {:reply, {:ok, %{payload: Base.encode64(reply_data)}}, socket}

          {:error, reason} ->
            {:reply, {:error, %{reason: to_string(reason)}}, socket}
        end
      else
        :error ->
          {:reply, {:error, %{reason: "invalid_base64"}}, socket}
      end
    end
  end

  @impl true
  def handle_info({:DOWN, ref, :process, _pid, _reason}, socket) do
    if ref == socket.assigns.actor_ref do
      push(socket, "actor_stopped", %{})
      {:stop, :normal, socket}
    else
      {:noreply, socket}
    end
  end

  def handle_info(%{event: "state_update", payload: payload}, socket) do
    push(socket, "state_update", payload)
    {:noreply, socket}
  end

  def handle_info(%{event: "node_draining"}, socket) do
    push(socket, "node_draining", %{})
    {:noreply, socket}
  end

  def handle_info(_msg, socket) do
    {:noreply, socket}
  end

  defp parse_actor_topic(rest) do
    case String.split(rest, ":") do
      [namespace, type, id] ->
        actor_id = %ActorId{namespace: namespace, type: type, id: id}

        case ActorId.validate(actor_id) do
          :ok -> {:ok, actor_id}
          {:error, _} -> :error
        end

      _ ->
        :error
    end
  end
end
