defmodule Quanta.Web.ChannelHelpers do
  @moduledoc false

  import Phoenix.Channel, only: [push: 3]

  alias Quanta.Actor.Server
  alias Quanta.{ActorId, Envelope}
  alias Quanta.Web.Presence

  @max_user_id_len 128
  @user_id_pattern ~r/^[a-zA-Z0-9_-]+$/

  @spec parse_actor_topic(String.t()) :: {:ok, ActorId.t()} | :error
  def parse_actor_topic(rest) do
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

  @spec resolve_user_id(map(), Phoenix.Socket.t()) :: String.t()
  def resolve_user_id(%{"user_id" => id}, _socket)
      when is_binary(id) and byte_size(id) > 0 and byte_size(id) <= @max_user_id_len do
    if Regex.match?(@user_id_pattern, id), do: id, else: generate_user_id()
  end

  def resolve_user_id(_params, _socket), do: generate_user_id()

  @spec generate_user_id() :: String.t()
  def generate_user_id, do: Base.url_encode64(:crypto.strong_rand_bytes(12), padding: false)

  @spec dispatch_message(String.t(), Phoenix.Socket.t()) ::
          {:reply, {:ok, map()} | {:error, map()}, Phoenix.Socket.t()}
  def dispatch_message(payload_b64, socket) do
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

  @spec handle_actor_down(reference(), Phoenix.Socket.t()) ::
          {:stop, :normal, Phoenix.Socket.t()} | {:noreply, Phoenix.Socket.t()}
  def handle_actor_down(ref, socket) do
    if ref == socket.assigns.actor_ref do
      push(socket, "actor_stopped", %{})
      {:stop, :normal, socket}
    else
      {:noreply, socket}
    end
  end

  @spec push_presence_state(Phoenix.Socket.t()) :: {:noreply, Phoenix.Socket.t()}
  def push_presence_state(socket) do
    push(socket, "presence_state", Presence.list(socket.topic))
    {:noreply, socket}
  end
end
