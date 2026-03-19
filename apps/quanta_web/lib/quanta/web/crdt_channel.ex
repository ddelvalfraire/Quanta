defmodule Quanta.Web.CrdtChannel do
  use Phoenix.Channel

  alias Quanta.Actor.{CommandRouter, Server}
  alias Quanta.Web.{ChannelHelpers, Presence}

  @max_delta_bytes 1_048_576

  @impl true
  def join("crdt:" <> rest, params, socket) do
    case ChannelHelpers.parse_actor_topic(rest) do
      {:ok, actor_id} ->
        if actor_id.namespace != socket.assigns.auth_namespace do
          {:error, %{reason: "namespace_forbidden"}}
        else
          with {:ok, pid} <- CommandRouter.ensure_active(actor_id),
               {:ok, snapshot} <- Server.get_crdt_snapshot(pid) do
            ref = Process.monitor(pid)
            user_id = ChannelHelpers.resolve_user_id(params, socket)
            :ok = Server.subscribe(pid, self(), user_id)

            socket =
              socket
              |> assign(:actor_id, actor_id)
              |> assign(:actor_pid, pid)
              |> assign(:actor_ref, ref)
              |> assign(:user_id, user_id)

            Phoenix.PubSub.subscribe(Quanta.Web.PubSub, "system:drain")

            topic = "crdt:#{actor_id.namespace}:#{actor_id.type}:#{actor_id.id}"
            Presence.track(self(), topic, user_id, %{joined_at: System.system_time(:second)})
            send(self(), :after_join)

            {:ok, %{snapshot: Base.encode64(snapshot)}, socket}
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
  def handle_in("crdt_update", %{"delta" => delta_b64}, socket) do
    if socket.assigns.auth_scope == :ro do
      {:reply, {:error, %{reason: "insufficient_scope"}}, socket}
    else
      with {:ok, delta} <- Base.decode64(delta_b64),
           true <- byte_size(delta) <= @max_delta_bytes do
        GenServer.cast(socket.assigns.actor_pid, {:crdt_delta, delta, socket.assigns.user_id})
        {:reply, {:ok, %{}}, socket}
      else
        false ->
          {:reply, {:error, %{reason: "delta_too_large"}}, socket}

        :error ->
          {:reply, {:error, %{reason: "invalid_base64"}}, socket}
      end
    end
  end

  @impl true
  def handle_in("message", %{"payload" => payload_b64}, socket) do
    ChannelHelpers.dispatch_message(payload_b64, socket)
  end

  @impl true
  def handle_info({:crdt_update, delta_bytes, peer_id}, socket) do
    if peer_id == socket.assigns.user_id do
      {:noreply, socket}
    else
      push(socket, "crdt_update", %{delta: Base.encode64(delta_bytes), peer_id: peer_id})
      {:noreply, socket}
    end
  end

  @impl true
  def handle_info({:DOWN, ref, :process, _pid, _reason}, socket) do
    ChannelHelpers.handle_actor_down(ref, socket)
  end

  @impl true
  def handle_info(:after_join, socket) do
    ChannelHelpers.push_presence_state(socket)
  end

  @impl true
  def handle_info(:node_draining, socket) do
    push(socket, "node_draining", %{reconnect_ms: 1_000})
    {:noreply, socket}
  end

  @impl true
  def handle_info(_msg, socket) do
    {:noreply, socket}
  end

  @impl true
  def terminate(_reason, socket) do
    if pid = socket.assigns[:actor_pid] do
      try do
        Server.unsubscribe(pid, self())
      catch
        :exit, _ -> :ok
      end
    end

    :ok
  end
end
