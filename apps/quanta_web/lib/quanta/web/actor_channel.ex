defmodule Quanta.Web.ActorChannel do
  use Phoenix.Channel

  alias Quanta.Actor.{CommandRouter, Server}
  alias Quanta.{ActorId, Envelope}
  alias Quanta.Web.Presence

  @impl true
  def join("actor:" <> rest, params, socket) do
    case parse_actor_topic(rest) do
      {:ok, actor_id} ->
        if actor_id.namespace != socket.assigns.auth_namespace do
          {:error, %{reason: "namespace_forbidden"}}
        else
          with {:ok, pid} <- CommandRouter.ensure_active(actor_id),
               {:ok, state_data} <- fetch_state(pid) do
            ref = Process.monitor(pid)
            user_id = resolve_user_id(params, socket)

            socket =
              socket
              |> assign(:actor_id, actor_id)
              |> assign(:actor_pid, pid)
              |> assign(:actor_ref, ref)
              |> assign(:user_id, user_id)

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

  @impl true
  def handle_info(%{event: "state_update", payload: payload}, socket) do
    push(socket, "state_update", payload)
    {:noreply, socket}
  end

  @impl true
  def handle_info(%{event: "node_draining"}, socket) do
    push(socket, "node_draining", %{})
    {:noreply, socket}
  end

  @impl true
  def handle_info(:after_join, socket) do
    topic = socket.topic
    push(socket, "presence_state", Presence.list(topic))
    {:noreply, socket}
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

  @max_user_id_len 128
  @user_id_pattern ~r/^[a-zA-Z0-9_-]+$/

  defp resolve_user_id(%{"user_id" => id}, _socket)
       when is_binary(id) and byte_size(id) > 0 and byte_size(id) <= @max_user_id_len do
    if Regex.match?(@user_id_pattern, id), do: id, else: generate_user_id()
  end

  defp resolve_user_id(_params, _socket), do: generate_user_id()

  defp generate_user_id, do: Base.url_encode64(:crypto.strong_rand_bytes(12), padding: false)

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
