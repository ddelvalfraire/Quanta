defmodule Quanta.Actor.CommandRouter do
  @moduledoc """
  Routes commands to actors — via NATS subscription or direct `route/3` call.

  Subscribes to `quanta.*.cmd.*.*` with a queue group so that commands are
  load-balanced across nodes. When NATS is unavailable (or in HTTP-only mode),
  `route/3` can be called directly.

  In a multi-node cluster, consults the hash ring to forward activation
  to the owning node. Duplicate activations from fallback are safe because
  Syn conflict resolution + NATS KV fencing deduplicate them.
  """

  use GenServer

  alias Quanta.Actor.{DynSup, ManifestRegistry, Registry, Server}
  alias Quanta.{ActorId, Envelope, RateLimit}

  require Logger

  @nats_subject "quanta.*.cmd.*.*"
  @queue_group "quanta-cmd-router"
  @default_max_actors 1_000_000
  @ensure_active_rpc_timeout 10_000

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
      {:manifest, :error} ->
        {:error, :actor_type_not_found}

      {:rate, {:error, :rate_limited, _retry_after}} ->
        :telemetry.execute(
          [:quanta, :rate_limit, :rejected],
          %{},
          %{actor_id: actor_id}
        )

        {:error, :rate_limited}
    end
  end

  @doc """
  Look up an actor, activating it if not already running.

  Skips rate limits — this is for establishing a channel connection,
  not delivering a message. The node capacity guard still applies.
  """
  @spec ensure_active(ActorId.t()) :: {:ok, pid()} | {:error, term()}
  def ensure_active(%ActorId{} = actor_id) do
    case Registry.lookup(actor_id) do
      {:ok, pid} ->
        {:ok, pid}

      :not_found ->
        target = safe_target_node(actor_id)

        if target == node() do
          ensure_active_locally(actor_id)
        else
          case :rpc.call(target, __MODULE__, :ensure_active_local, [actor_id], @ensure_active_rpc_timeout) do
            {:badrpc, reason} ->
              Logger.warning(
                "RPC ensure_active to #{target} failed: #{inspect(reason)}, falling back to local"
              )

              ensure_active_locally(actor_id)

            result ->
              result
          end
        end
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

  @doc false
  def route_local(%ActorId{} = actor_id, %Envelope{} = envelope, timeout) do
    with {:manifest, {:ok, manifest}} <-
           {:manifest, ManifestRegistry.get(actor_id.namespace, actor_id.type)},
         {:rate, :ok} <- {:rate, RateLimit.check(actor_id, manifest)} do
      case Registry.lookup(actor_id) do
        {:ok, pid} -> deliver(pid, envelope, timeout)
        :not_found -> activate_locally_and_deliver(actor_id, manifest, envelope, timeout)
      end
    else
      {:manifest, :error} ->
        {:error, :actor_type_not_found}

      {:rate, {:error, :rate_limited, _retry_after}} ->
        :telemetry.execute(
          [:quanta, :rate_limit, :rejected],
          %{},
          %{actor_id: actor_id}
        )

        {:error, :rate_limited}
    end
  end

  @doc false
  def ensure_active_local(%ActorId{} = actor_id) do
    case Registry.lookup(actor_id) do
      {:ok, pid} -> {:ok, pid}
      :not_found -> ensure_active_locally(actor_id)
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
    target = safe_target_node(actor_id)

    if target == node() do
      activate_locally_and_deliver(actor_id, manifest, envelope, timeout)
    else
      case :rpc.call(target, __MODULE__, :route_local, [actor_id, envelope, timeout], timeout) do
        {:badrpc, reason} ->
          Logger.warning(
            "RPC route to #{target} failed: #{inspect(reason)}, falling back to local"
          )

          activate_locally_and_deliver(actor_id, manifest, envelope, timeout)

        result ->
          result
      end
    end
  end

  defp activate_locally_and_deliver(actor_id, manifest, envelope, timeout) do
    with :ok <- check_capacity() do
      case Quanta.ModuleResolver.resolve(manifest) do
        {:error, :module_not_configured} ->
          {:error, :module_not_configured}

        {:ok, module} ->
          start_and_deliver(actor_id, module, envelope, timeout)
      end
    end
  end

  defp start_and_deliver(actor_id, module, envelope, timeout) do
    case start_actor(actor_id, module) do
      {:ok, pid} -> deliver(pid, envelope, timeout)
      {:error, reason} -> {:error, reason}
    end
  end

  defp start_actor(actor_id, module) do
    opts = [actor_id: actor_id, module: module]

    case DynSup.start_actor(actor_id, opts) do
      {:ok, pid} ->
        {:ok, pid}

      {:error, {:already_started, pid}} ->
        {:ok, pid}

      {:error, {:already_registered, _actor_id}} ->
        case Registry.lookup(actor_id) do
          {:ok, pid} -> {:ok, pid}
          :not_found -> {:error, :activation_race_lost}
        end

      {:error, reason} ->
        {:error, reason}
    end
  end

  defp deliver(pid, envelope, timeout) do
    Server.send_message(pid, envelope, timeout)
  end

  defp ensure_active_locally(actor_id) do
    with :ok <- check_capacity(),
         {:manifest, {:ok, manifest}} <-
           {:manifest, ManifestRegistry.get(actor_id.namespace, actor_id.type)},
         {:resolve, {:ok, module}} <-
           {:resolve, Quanta.ModuleResolver.resolve(manifest)} do
      start_actor(actor_id, module)
    else
      {:error, :node_at_capacity} -> {:error, :node_at_capacity}
      {:manifest, :error} -> {:error, :actor_type_not_found}
      {:resolve, {:error, reason}} -> {:error, reason}
    end
  end

  defp safe_target_node(actor_id) do
    case Quanta.Cluster.Topology.ring() do
      {:ok, ring} ->
        key = ActorId.to_placement_key(actor_id)
        {:ok, target} = ExHashRing.Ring.find_node(ring, key)
        target

      {:error, :not_ready} ->
        Logger.debug("Hash ring not ready, routing #{inspect(actor_id)} locally")
        node()
    end
  end

  defp check_capacity do
    max = Application.get_env(:quanta_distributed, :max_actors_per_node, @default_max_actors)

    if DynSup.count_actors() >= max do
      {:error, :node_at_capacity}
    else
      :ok
    end
  end

  @impl true
  def init(_opts) do
    state = %{subscription: nil}

    if Process.whereis(Quanta.Nats.Core.connection(0)) do
      {:ok, state, {:continue, :subscribe}}
    else
      Logger.info("CommandRouter started in HTTP-only mode (no NATS connection)")
      {:ok, state}
    end
  end

  @impl true
  def handle_continue(:subscribe, state) do
    case Quanta.Nats.Core.subscribe(@nats_subject, @queue_group, self()) do
      {:ok, sub} ->
        Logger.info("CommandRouter subscribed to #{@nats_subject}")
        {:noreply, %{state | subscription: sub}}

      {:error, reason} ->
        Logger.error("CommandRouter failed to subscribe: #{inspect(reason)}")
        Process.send_after(self(), :retry_subscribe, 5_000)
        {:noreply, state}
    end
  catch
    :exit, reason ->
      Logger.warning("CommandRouter NATS not ready, retrying: #{inspect(reason)}")
      Process.send_after(self(), :retry_subscribe, 5_000)
      {:noreply, state}
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

    if reply_to, do: send_nats_reply(reply_to, result)

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

  defp send_nats_reply(reply_to, result) do
    Quanta.Nats.Core.publish(reply_to, encode_reply(result))
  catch
    :exit, reason ->
      Logger.warning("Failed to send NATS reply to #{reply_to}: #{inspect(reason)}")
  end

  defp encode_reply({:ok, data}) when is_binary(data), do: data
  defp encode_reply({:ok, :no_reply}), do: <<>>
  defp encode_reply({:error, reason}), do: "error:" <> to_string(reason)
end
