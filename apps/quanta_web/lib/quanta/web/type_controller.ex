defmodule Quanta.Web.TypeController do
  use Phoenix.Controller, formats: [:json]

  import Quanta.Web.ErrorHelpers

  alias Quanta.Actor.ManifestRegistry
  alias Quanta.Manifest
  alias Quanta.SchemaEvolution

  plug Quanta.Web.Plugs.RequireScope, :admin when action in [:deploy]
  plug Quanta.Web.Plugs.RequireScope, :ro when action in [:list_types]

  def list_types(conn, %{"ns" => ns}) do
    manifests = ManifestRegistry.list_types(ns)

    types =
      Enum.map(manifests, fn m ->
        %{
          namespace: m.namespace,
          type: m.type,
          version: m.version
        }
      end)

    json(conn, types)
  end

  def deploy(conn, %{"ns" => ns, "type" => type}) do
    cond do
      conn.body_params["wasm"] ->
        error_response(conn, :wasm_not_available)

      upload = conn.body_params["manifest"] ->
        wit_upload = conn.body_params["wit"]
        deploy_manifest(conn, upload, wit_upload, ns, type)

      true ->
        error_response(conn, 400, "missing manifest part")
    end
  end

  defp deploy_manifest(conn, %Plug.Upload{path: path}, wit_upload, ns, type) do
    with {:ok, yaml} <- File.read(path),
         {:ok, manifest} <- Manifest.parse_yaml(yaml),
         :ok <- validate_route_match(manifest, ns, type),
         :ok <- check_schema_evolution(manifest, wit_upload, ns, type),
         :ok <- ManifestRegistry.put(manifest) do
      json(conn, %{
        namespace: manifest.namespace,
        type: manifest.type,
        version: manifest.version
      })
    else
      {:error, errors} when is_list(errors) ->
        error_response(conn, 422, "manifest validation failed", errors)

      {:error, reason} when is_binary(reason) ->
        error_response(conn, 422, reason)

      {:error, reason} ->
        error_response(conn, reason)
    end
  end

  defp check_schema_evolution(manifest, wit_upload, ns, type) do
    case {manifest.state.kind, wit_upload} do
      {{:schematized, type_name}, %Plug.Upload{path: wit_path}} ->
        with {:ok, wit_source} <- File.read(wit_path) do
          prev_version = get_previous_state_version(ns, type)
          SchemaEvolution.check_deploy(manifest, wit_source, type_name, prev_version)
        end

      _ ->
        :ok
    end
  end

  defp get_previous_state_version(ns, type) do
    case ManifestRegistry.get(ns, type) do
      {:ok, prev} -> prev.state.version
      :error -> nil
    end
  end

  defp validate_route_match(manifest, ns, type) do
    cond do
      manifest.namespace != ns ->
        {:error, "manifest namespace #{inspect(manifest.namespace)} does not match URL namespace #{inspect(ns)}"}

      manifest.type != type ->
        {:error, "manifest type #{inspect(manifest.type)} does not match URL type #{inspect(type)}"}

      true ->
        :ok
    end
  end
end
