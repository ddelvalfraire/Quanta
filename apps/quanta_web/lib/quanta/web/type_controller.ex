defmodule Quanta.Web.TypeController do
  use Phoenix.Controller, formats: [:json]

  import Quanta.Web.ErrorHelpers

  alias Quanta.Actor.ManifestRegistry
  alias Quanta.Manifest

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
        deploy_manifest(conn, upload, ns, type)

      true ->
        error_response(conn, 400, "missing manifest part")
    end
  end

  defp deploy_manifest(conn, %Plug.Upload{path: path}, ns, type) do
    with {:ok, yaml} <- File.read(path),
         {:ok, manifest} <- Manifest.parse_yaml(yaml),
         :ok <- validate_route_match(manifest, ns, type),
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
