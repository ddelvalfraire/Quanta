defmodule Quanta.SchemaEvolution do
  @moduledoc """
  Deploy-time schema compatibility check for schematized actors.

  Ensures append-only evolution: existing fields must not change name or
  type, and fields may only be added at the end. If a breaking change is
  required, the manifest's `state.version` must be incremented to
  acknowledge the incompatibility.

  Stores exported schema bytes in an ETS table keyed by `{namespace, type}`
  so subsequent deploys can compare against the previous schema.
  """

  alias Quanta.Nifs.SchemaCompiler

  @table __MODULE__

  @doc """
  Creates the ETS table. Safe to call multiple times — no-ops if table exists.
  """
  def create_table do
    if :ets.whereis(@table) == :undefined do
      :ets.new(@table, [:named_table, :set, :public, read_concurrency: true])
    end

    :ok
  end

  @doc """
  Check schema compatibility for a deploy.

  Returns `:ok` if the deploy should proceed, or `{:error, reason}` if rejected.
  On success, caches the new schema's exported bytes for future comparisons.
  """
  @spec check_deploy(
          manifest :: Quanta.Manifest.t(),
          wit_source :: String.t(),
          type_name :: String.t(),
          previous_state_version :: pos_integer() | nil
        ) :: :ok | {:error, String.t()}
  def check_deploy(manifest, wit_source, type_name, previous_state_version) do
    with {:ok, new_ref, _warnings} <- compile_or_error(wit_source, type_name) do
      previous_bytes = get_cached_schema(manifest.namespace, manifest.type)
      check_and_cache(manifest, new_ref, previous_bytes, previous_state_version)
    end
  end

  defp compile_or_error(wit_source, type_name) do
    case SchemaCompiler.compile(wit_source, type_name) do
      {:ok, _ref, _warnings} = ok -> ok
      {:error, reason} -> {:error, "schema compilation failed: #{reason}"}
    end
  end

  defp check_and_cache(manifest, new_ref, nil, _prev_version) do
    # First deploy — no previous schema. Cache and proceed.
    cache_schema(manifest.namespace, manifest.type, new_ref)
  end

  defp check_and_cache(manifest, new_ref, previous_bytes, prev_version) do
    with {:ok, old_ref} <- import_or_error(previous_bytes),
         {:ok, result, details} <- SchemaCompiler.check_compatibility(old_ref, new_ref) do
      case result do
        r when r in [:identical, :compatible] ->
          cache_schema(manifest.namespace, manifest.type, new_ref)

        :incompatible ->
          if manifest.state.version > (prev_version || 0) do
            cache_schema(manifest.namespace, manifest.type, new_ref)
          else
            {:error,
             "schema incompatible: #{details}. Increment state.version to acknowledge."}
          end
      end
    end
  end

  defp import_or_error(bytes) do
    case SchemaCompiler.import_schema(bytes) do
      {:ok, _ref} = ok -> ok
      {:error, reason} -> {:error, "failed to import previous schema: #{reason}"}
    end
  end

  defp cache_schema(namespace, type, schema_ref) do
    {:ok, bytes} = SchemaCompiler.export(schema_ref)
    :ets.insert(@table, {{namespace, type}, bytes})
    :ok
  end

  defp get_cached_schema(namespace, type) do
    case :ets.lookup(@table, {namespace, type}) do
      [{_key, bytes}] -> bytes
      [] -> nil
    end
  end

  @doc false
  def reset_table do
    :ets.delete_all_objects(@table)
    :ok
  end
end
