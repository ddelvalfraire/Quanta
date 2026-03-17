defmodule Quanta.Actor.CommandRouter do
  @moduledoc """
  Routes commands to actors — via NATS subscription or direct `route/3` call.

  Subscribes to `quanta.*.cmd.*.*` with a queue group so that commands are
  load-balanced across nodes. When NATS is unavailable (or in HTTP-only mode),
  `route/3` can be called directly.

  Routing algorithm:
    1. Look up manifest via ManifestRegistry
    2. Check rate limits
    3. Find actor in Syn registry (or activate via DynSup)
    4. Deliver message via GenServer.call
  """

  use GenServer

  alias Quanta.Actor.{DynSup, ManifestRegistry, Registry, Server}
  alias Quanta.{ActorId, Envelope, RateLimit}

  require Logger

  @nats_subject "quanta.*.cmd.*.*"
  @queue_group "quanta-cmd-router"

  @spec start_link(keyword()) :: GenServer.on_start()
  def start_link(opts \\ []) do
    GenServer.start_link(__MODULE__, opts, name: __MODULE__)
  end

  @doc "Route a message to an actor. Works without NATS (HTTP-only mode)."
  @spec route(ActorId.t(), Envelope.t(), pos_integer()) ::
          {:ok, binary()} | {:ok, :no_reply} | {:error, term()}
  def route(%ActorId{} = actor_id, %Envelope{} = envelope, timeout \\ 30_000) do
    with {:manifest, {:ok, manifest}} <-
           {:manifest, ManifestRegistry.get(actor_id.namespace, actor_id.type)},
         {:rate, :ok} <- {:rate, RateLimit.check(actor_id, manifest)} do
      find_and_deliver(actor_id, manifest, envelope, timeout)
    else
      {:manifest, :error} -> {:error, :actor_type_not_found}
      {:rate, {:error, :rate_limited, _retry_after}} -> {:error, :rate_limited}
    end
  end

  @doc """
  Parse a NATS command subject into an ActorId.

  Expected format: `quanta.{namespace}.cmd.{type}.{id}`
  """
  @spec parse_command_subject(String.t()) :: {:ok, ActorId.t()} | {:error, String.t()}
  def parse_command_subject(subject) do
    case String.split(subject, ".") do
      ["quanta", namespace, "cmd", type, id] ->
        actor_id = %ActorId{namespace: namespace, type: type, id: id}

        case ActorId.validate(actor_id) do
          :ok -> {:ok, actor_id}
          {:error, reason} -> {:error, reason}
        end

      _ ->
        {:error, "subject does not match quanta.{ns}.cmd.{type}.{id}"}
    end
  end

  defp find_and_deliver(actor_id, manifest, envelope, timeout) do
    case Registry.lookup(actor_id) do
      {:ok, pid} ->
        deliver(pid, envelope, timeout)

      :not_found ->
        activate_and_deliver(actor_id, manifest, envelope, timeout)
    end
  end

  defp activate_and_deliver(actor_id, manifest, envelope, timeout) do
    max_actors = Application.get_env(:quanta_distributed, :max_actors_per_node, 1_000_000)

    if DynSup.count_actors() >= max_actors do
      {:error, :node_at_capacity}
    else
      case manifest_module(manifest) do
        nil ->
          {:error, :module_not_configured}

        module ->
          start_and_deliver(actor_id, module, envelope, timeout)
      end
    end
  end

  defp start_and_deliver(actor_id, module, envelope, timeout) do
    opts = [actor_id: actor_id, module: module]

    case DynSup.start_actor(actor_id, opts) do
      {:ok, pid} ->
        deliver(pid, envelope, timeout)

      {:error, {:already_started, pid}} ->
        deliver(pid, envelope, timeout)

      {:error, {:already_registered, _actor_id}} ->
        case Registry.lookup(actor_id) do
          {:ok, pid} -> deliver(pid, envelope, timeout)
          :not_found -> {:error, :activation_race_lost}
        end

      {:error, reason} ->
        {:error, reason}
    end
  end

  defp deliver(pid, envelope, timeout) do
    Server.send_message(pid, envelope, timeout)
  end

  # Phase 1: resolve module from app config; T06/T16 will use WASM module registry.
  defp manifest_module(manifest) do
    case Application.get_env(:quanta_distributed, :actor_modules, %{}) do
      modules when is_map(modules) ->
        Map.get(modules, {manifest.namespace, manifest.type})

      _ ->
        nil
    end
  end

  @impl true
  def init(opts) do
    nats_connection = Keyword.get(opts, :nats_connection)

    state = %{
      nats_connection: nats_connection,
      subscription: nil
    }

    if nats_connection do
      {:ok, state, {:continue, :subscribe}}
    else
      Logger.info("CommandRouter started in HTTP-only mode (no NATS connection)")
      {:ok, state}
    end
  end

  @impl true
  def handle_continue(:subscribe, state) do
    case subscribe(state.nats_connection) do
      {:ok, sid} ->
        Logger.info("CommandRouter subscribed to #{@nats_subject}")
        {:noreply, %{state | subscription: sid}}

      {:error, reason} ->
        Logger.error("CommandRouter failed to subscribe: #{inspect(reason)}")
        Process.send_after(self(), :retry_subscribe, 5_000)
        {:noreply, state}
    end
  end

  # NATS messages are dispatched synchronously in the GenServer process.
  # This serializes throughput to one message at a time (up to timeout).
  # Acceptable for Phase 1; T09 integration should spawn into a Task pool.
  @impl true
  def handle_info({:msg, %{topic: topic, body: body, reply_to: reply_to}}, state) do
    result =
      case parse_command_subject(topic) do
        {:ok, actor_id} ->
          envelope = Envelope.new(payload: body, sender: {:client, "nats"})
          route(actor_id, envelope)

        {:error, reason} ->
          Logger.warning("CommandRouter: invalid subject #{topic}: #{reason}")
          {:error, :invalid_subject}
      end

    if reply_to, do: send_nats_reply(state.nats_connection, reply_to, result)

    {:noreply, state}
  end

  @impl true
  def handle_info(:retry_subscribe, state) do
    {:noreply, state, {:continue, :subscribe}}
  end

  @impl true
  def handle_info(_msg, state) do
    {:noreply, state}
  end

  defp subscribe(connection) do
    Gnat.sub(connection, self(), @nats_subject, queue_group: @queue_group)
  end

  defp send_nats_reply(connection, reply_to, result) do
    Gnat.pub(connection, reply_to, encode_reply(result))
  end

  defp encode_reply({:ok, data}) when is_binary(data), do: data
  defp encode_reply({:ok, :no_reply}), do: <<>>
  defp encode_reply({:error, reason}), do: "error:" <> to_string(reason)
end
