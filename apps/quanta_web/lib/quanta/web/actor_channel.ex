defmodule Quanta.Web.ActorChannel do
  use Phoenix.Channel

  alias Quanta.Actor.{CommandRouter, Server}
  alias Quanta.Nifs.DeltaEncoder
  alias Quanta.Web.{ChannelHelpers, Presence}

  @impl true
  def join("actor:" <> rest, params, socket) do
    case ChannelHelpers.parse_actor_topic(rest) do
      {:ok, actor_id} ->
        if actor_id.namespace != socket.assigns.auth_namespace do
          {:error, %{reason: "namespace_forbidden"}}
        else
          format = Map.get(params, "format", "binary")

          if format not in ["binary", "json"] do
            {:error, %{reason: "invalid_format"}}
          else
            join_actor(actor_id, params, format, socket)
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
  def handle_in("input", %{"input_seq" => input_seq} = params, socket)
      when is_integer(input_seq) and input_seq >= 0 do
    if socket.assigns.auth_scope == :ro do
      {:reply, {:error, %{reason: "insufficient_scope"}}, socket}
    else
      if socket.assigns[:prediction] do
        socket = assign(socket, :last_input_seq, input_seq)

        case Map.get(params, "data") do
          nil ->
            {:reply, :ok, socket}

          data when is_binary(data) ->
            envelope = Quanta.Envelope.new(payload: data, sender: {:client, "channel"})
            Server.send_message(socket.assigns.actor_pid, envelope)
            {:reply, :ok, socket}

          _ ->
            {:reply, :ok, socket}
        end
      else
        {:reply, {:error, %{reason: "prediction_not_enabled"}}, socket}
      end
    end
  end

  @impl true
  def handle_info({:DOWN, ref, :process, _pid, _reason}, socket) do
    ChannelHelpers.handle_actor_down(ref, socket)
  end

  @impl true
  def handle_info({:delta_update, delta, new_state_data, seq, schema_version}, socket) do
    schema_ref = socket.assigns[:schema_ref]

    socket =
      cond do
        is_nil(schema_ref) || socket.assigns[:schema_version] != schema_version ->
          push_full_state(socket, new_state_data, seq, schema_version)
          assign(socket, :schema_version, schema_version)

        socket.assigns[:format] == "binary" ->
          payload = %{delta: Base.encode64(delta), seq: seq}
          payload = maybe_add_prediction_fields(payload, socket)
          push(socket, "delta", payload)
          socket

        socket.assigns[:format] == "json" ->
          with {:ok, decoded} <- DeltaEncoder.decode_state(schema_ref, new_state_data),
               {:ok, changed} <- DeltaEncoder.changed_fields(schema_ref, delta) do
            payload = %{state: decoded, changed: changed, seq: seq}
            payload = maybe_add_prediction_fields(payload, socket)
            push(socket, "delta", payload)
            socket
          else
            _ ->
              push_full_state(socket, new_state_data, seq, schema_version)
              socket
          end
      end

    {:noreply, assign(socket, :delta_seq, seq)}
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

  defp join_actor(actor_id, params, format, socket) do
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
        |> assign(:format, format)
        |> assign(:delta_seq, 0)
        |> assign(:last_input_seq, 0)
        |> assign(:prediction, false)

      Server.subscribe(pid, self(), user_id)
      Phoenix.PubSub.subscribe(Quanta.Web.PubSub, "system:drain")

      topic = "actor:#{actor_id.namespace}:#{actor_id.type}:#{actor_id.id}"
      Presence.track(self(), topic, user_id, %{joined_at: System.system_time(:second)})
      send(self(), :after_join)

      {reply, socket} = build_join_reply(pid, format, state_data, socket)
      {:ok, reply, socket}
    else
      {:error, reason} ->
        {:error, %{reason: to_string(reason)}}
    end
  end

  defp build_join_reply(pid, format, state_data, socket) do
    case Server.get_schema_info(pid) do
      {:ok, schema_ref, schema_bytes, schema_version} ->
        socket =
          socket
          |> assign(:schema_ref, schema_ref)
          |> assign(:schema_version, schema_version)

        case format do
          "binary" ->
            reply = %{
              schema: Base.encode64(schema_bytes),
              state: Base.encode64(state_data),
              schema_version: schema_version,
              seq: 0
            }

            {reply, socket}

          "json" ->
            case DeltaEncoder.decode_state(schema_ref, state_data) do
              {:ok, decoded} ->
                reply = %{
                  state: decoded,
                  schema_version: schema_version,
                  seq: 0
                }

                {reply, socket}

              {:error, _} ->
                {%{state: Base.encode64(state_data)}, socket}
            end
        end

      {:error, :no_schema} ->
        {%{state: Base.encode64(state_data)}, socket}
    end
  catch
    :exit, _ ->
      {%{state: Base.encode64(state_data)}, socket}
  end

  defp push_full_state(socket, state_data, seq, schema_version) do
    case socket.assigns[:format] do
      "binary" ->
        push(socket, "snapshot", %{
          state: Base.encode64(state_data),
          schema_version: schema_version,
          seq: seq
        })

      "json" ->
        schema_ref = socket.assigns[:schema_ref]

        case DeltaEncoder.decode_state(schema_ref, state_data) do
          {:ok, decoded} ->
            push(socket, "snapshot", %{
              state: decoded,
              schema_version: schema_version,
              seq: seq
            })

          _ ->
            push(socket, "snapshot", %{
              state: Base.encode64(state_data),
              schema_version: schema_version,
              seq: seq
            })
        end
    end
  end

  defp maybe_add_prediction_fields(payload, socket) do
    if socket.assigns[:prediction] do
      payload
      |> Map.put(:server_tick, System.monotonic_time(:millisecond))
      |> Map.put(:last_input_seq, socket.assigns[:last_input_seq])
    else
      payload
    end
  end

  defp fetch_state(pid) do
    Server.get_state(pid)
  catch
    :exit, _ -> {:error, :actor_unavailable}
  end
end
