defmodule Quanta.Web.ActorController do
  use Phoenix.Controller, formats: [:json]

  import Quanta.Web.ErrorHelpers

  alias Quanta.Actor.{CommandRouter, DynSup, ManifestRegistry, Registry, Server}
  alias Quanta.{ActorId, Envelope, ModuleResolver}

  plug Quanta.Web.Plugs.RequireScope, :rw when action in [:send_message, :spawn, :destroy]
  plug Quanta.Web.Plugs.RequireScope, :ro when action in [:get_state, :get_meta]

  @http_timeout_ms 5_000
  @max_body_bytes 1_048_576

  def send_message(conn, %{"ns" => ns, "type" => type, "id" => id}) do
    with {:ok, actor_id} <- build_actor_id(ns, type, id),
         {:ok, body, conn} <- read_payload(conn) do
      correlation_id = get_req_header(conn, "x-quanta-correlation-id") |> List.first()

      envelope =
        Envelope.new(
          payload: body,
          sender: {:client, "http"},
          correlation_id: correlation_id
        )

      case CommandRouter.route(actor_id, envelope, @http_timeout_ms) do
        {:ok, binary} when is_binary(binary) ->
          conn
          |> put_resp_content_type("application/octet-stream")
          |> send_resp(200, binary)

        {:ok, :no_reply} ->
          send_resp(conn, 202, "")

        {:error, reason} ->
          error_response(conn, reason)
      end
    else
      {:error, reason} -> error_response(conn, reason)
    end
  end

  def get_state(conn, %{"ns" => ns, "type" => type, "id" => id}) do
    with {:ok, actor_id} <- build_actor_id(ns, type, id),
         {:ok, pid} <- lookup_actor(actor_id),
         {:ok, state_data} <- call_actor(fn -> Server.get_state(pid) end) do
      conn
      |> put_resp_content_type("application/octet-stream")
      |> send_resp(200, state_data)
    else
      {:error, reason} -> error_response(conn, reason)
    end
  end

  def get_meta(conn, %{"ns" => ns, "type" => type, "id" => id}) do
    with {:ok, actor_id} <- build_actor_id(ns, type, id),
         {:ok, pid} <- lookup_actor(actor_id),
         {:ok, meta} <- call_actor(fn -> Server.get_meta(pid) end) do
      activated_at_iso =
        (meta.activated_at + System.time_offset(:native))
        |> System.convert_time_unit(:native, :microsecond)
        |> DateTime.from_unix!(:microsecond)
        |> DateTime.to_iso8601()

      json(conn, %{
        actor_id: %{
          namespace: meta.actor_id.namespace,
          type: meta.actor_id.type,
          id: meta.actor_id.id
        },
        status: meta.status,
        message_count: meta.message_count,
        activated_at: activated_at_iso
      })
    else
      {:error, reason} -> error_response(conn, reason)
    end
  end

  def spawn(conn, %{"ns" => ns, "type" => type}) do
    id = Map.get(conn.body_params, "id") || Quanta.ULID.generate()

    with {:ok, actor_id} <- build_actor_id(ns, type, id),
         {:ok, manifest} <- fetch_manifest(ns, type),
         {:ok, module} <- ModuleResolver.resolve(manifest),
         :ok <- check_capacity() do
      opts = [actor_id: actor_id, module: module]

      case DynSup.start_actor(actor_id, opts) do
        {:ok, _pid} ->
          conn
          |> put_status(201)
          |> json(%{actor_id: %{namespace: ns, type: type, id: id}})

        {:error, {:already_started, _pid}} ->
          error_response(conn, :actor_already_exists)

        {:error, {:already_registered, _}} ->
          error_response(conn, :actor_already_exists)

        {:error, _reason} ->
          error_response(conn, :node_at_capacity)
      end
    else
      {:error, reason} -> error_response(conn, reason)
    end
  end

  def destroy(conn, %{"ns" => ns, "type" => type, "id" => id}) do
    with {:ok, actor_id} <- build_actor_id(ns, type, id),
         {:ok, pid} <- lookup_actor(actor_id) do
      :ok = Server.force_passivate(pid)
      send_resp(conn, 204, "")
    else
      {:error, reason} -> error_response(conn, reason)
    end
  end

  defp build_actor_id(ns, type, id) do
    actor_id = %ActorId{namespace: ns, type: type, id: id}

    case ActorId.validate(actor_id) do
      :ok -> {:ok, actor_id}
      {:error, _} -> {:error, :invalid_actor_id}
    end
  end

  defp lookup_actor(actor_id) do
    case Registry.lookup(actor_id) do
      {:ok, pid} -> {:ok, pid}
      :not_found -> {:error, :actor_not_found}
    end
  end

  defp fetch_manifest(ns, type) do
    case ManifestRegistry.get(ns, type) do
      {:ok, manifest} -> {:ok, manifest}
      :error -> {:error, :actor_type_not_found}
    end
  end

  defp check_capacity do
    max = Application.get_env(:quanta_distributed, :max_actors_per_node, 1_000_000)

    if DynSup.count_actors() < max do
      :ok
    else
      {:error, :node_at_capacity}
    end
  end

  defp read_payload(conn) do
    case Plug.Conn.read_body(conn, length: @max_body_bytes) do
      {:ok, body, conn} -> {:ok, body, conn}
      {:more, _, _conn} -> {:error, :payload_too_large}
      {:error, _reason} -> {:error, :payload_too_large}
    end
  end

  defp call_actor(fun) do
    fun.()
  catch
    :exit, _ -> {:error, :actor_not_found}
  end
end
