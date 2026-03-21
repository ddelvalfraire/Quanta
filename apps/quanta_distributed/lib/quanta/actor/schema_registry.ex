defmodule Quanta.Actor.SchemaRegistry do
  @moduledoc """
  Persistent, distributed schema store backed by NATS KV.

  Enforces immutability — the same `{type, version}` must always map
  to the same WIT source.
  """

  require Logger

  @hash_bytes 32
  @default_max_versions 10

  @doc """
  Store a compiled schema. Idempotent when content matches.

  Returns `{:error, :immutability_violation, detail}` if the same
  version is stored with different content.
  """
  @spec store(String.t(), String.t(), pos_integer(), String.t(), binary()) ::
          :ok | {:error, :immutability_violation, String.t()} | {:error, String.t()}
  def store(namespace, type, version, wit_source, compiled_bytes) do
    bucket = bucket_name(namespace)
    key = key(type, version)
    hash = :crypto.hash(:sha256, wit_source)
    value = <<hash::binary-size(@hash_bytes), compiled_bytes::binary>>
    js = Quanta.Nats.JetStream.impl()

    # NOTE: kv_get + kv_put is not atomic. A concurrent store on another node
    # can race past this check. Use kv_create (CAS) when the JetStream
    # behaviour supports it to close this TOCTOU window.
    case js.kv_get(bucket, key) do
      {:ok, existing_value, _revision} ->
        if existing_value == value do
          :ok
        else
          {:error, :immutability_violation,
           "schema for #{type}:#{version} already exists with different content"}
        end

      {:error, :not_found} ->
        case js.kv_put(bucket, key, value) do
          {:ok, _revision} ->
            purge_old_versions(js, bucket, type, version)
            :ok

          {:error, reason} ->
            {:error, "failed to store schema: #{inspect(reason)}"}
        end

      {:error, reason} ->
        {:error, "failed to check existing schema: #{inspect(reason)}"}
    end
  end

  @spec fetch(String.t(), String.t(), pos_integer()) ::
          {:ok, binary()} | {:error, :not_found | String.t()}
  def fetch(namespace, type, version) do
    bucket = bucket_name(namespace)
    key = key(type, version)
    js = Quanta.Nats.JetStream.impl()

    case js.kv_get(bucket, key) do
      {:ok, <<_hash::binary-size(@hash_bytes), compiled_bytes::binary>>, _revision} ->
        {:ok, compiled_bytes}

      {:error, :not_found} ->
        {:error, :not_found}

      {:error, reason} ->
        {:error, reason}
    end
  end

  # --- Private helpers ---

  defp bucket_name(namespace), do: "quanta_#{namespace}_schemas"

  defp key(type, version), do: "#{type}:#{version}"

  defp max_versions do
    Application.get_env(:quanta_distributed, :schema_registry_max_versions, @default_max_versions)
  end

  defp purge_old_versions(js, bucket, type, current_version) do
    cutoff = current_version - max_versions()

    for v <- 1..max(cutoff, 0)//1 do
      case js.kv_delete(bucket, key(type, v)) do
        :ok -> :ok
        {:error, reason} -> Logger.warning("SchemaRegistry: failed to purge #{type}:#{v}: #{inspect(reason)}")
      end
    end
  end
end
