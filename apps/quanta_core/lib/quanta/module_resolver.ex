defmodule Quanta.ModuleResolver do
  @moduledoc false

  @spec resolve(Quanta.Manifest.t()) :: {:ok, module()} | {:error, :module_not_configured}
  def resolve(%Quanta.Manifest{} = manifest) do
    case Application.get_env(:quanta_distributed, :actor_modules, %{}) do
      modules when is_map(modules) ->
        case Map.get(modules, {manifest.namespace, manifest.type}) do
          nil -> {:error, :module_not_configured}
          module -> {:ok, module}
        end

      _ ->
        {:error, :module_not_configured}
    end
  end
end
