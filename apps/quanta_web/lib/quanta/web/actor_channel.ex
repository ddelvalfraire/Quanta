defmodule Quanta.Web.ActorChannel do
  use Phoenix.Channel

  alias Quanta.Actor.{CommandRouter, Server}
  alias Quanta.Web.{ChannelHelpers, Presence}

  @impl true
  def join("actor:" <> rest, params, socket) do
    case ChannelHelpers.parse_actor_topic(rest) do
      {:ok, actor_id} ->
        if actor_id.namespace != socket.assigns.auth_namespace do
          {:error, %{reason: "namespace_forbidden"}}
        else
          with {:ok, pid} <- CommandRouter.ensure_active(actor_id),
               {:ok, state_data} <- fetch_state(pid) do
            ref = Process.monitor(pid)
            user_id = ChannelHelpers.resolve_user_id(params, socket)

            socket =
              socket
              |> assign(:actor_id, actor_id)
              |> assign(:actor_pid, pid)
              |> assign(:actor_ref, ref)
              |> assign(:user_id, user_id)

            Phoenix.PubSub.subscribe(Quanta.Web.PubSub, "system:drain")

            topic = "actor:#{actor_id.namespace}:#{actor_id.type}:#{actor_id.id}"
            Presence.track(self(), topic, user_id, %{joined_at: System.system_time(:second)})
            send(self(), :after_join)

            {:ok, %{state: Base.encode64(state_data)}, socket}
          else
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
    ChannelHelpers.dispatch_message(payload_b64, socket)
  end

  @impl true
  def handle_info({:DOWN, ref, :process, _pid, _reason}, socket) do
    ChannelHelpers.handle_actor_down(ref, socket)
  end

  @impl true
  def handle_info(%{event: "state_update", payload: payload}, socket) do
    push(socket, "state_update", payload)
    {:noreply, socket}
  end

  @impl true
  def handle_info(:node_draining, socket) do
    push(socket, "node_draining", %{reconnect_ms: 1_000})
    {:noreply, socket}
  end

  @impl true
  def handle_info(:after_join, socket) do
    ChannelHelpers.push_presence_state(socket)
  end

  @impl true
  def handle_info(%{event: "presence_diff", payload: diff}, socket) do
    push(socket, "presence_diff", diff)

    if socket.assigns[:actor_pid] do
      for {user_id, _metas} <- diff.leaves do
        send(socket.assigns.actor_pid, {:subscriber_left, user_id})
      end
    end

    {:noreply, socket}
  end

  @impl true
  def handle_info(_msg, socket) do
    {:noreply, socket}
  end

  defp fetch_state(pid) do
    Server.get_state(pid)
  catch
    :exit, _ -> {:error, :actor_unavailable}
  end
end
