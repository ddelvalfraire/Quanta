defmodule Quanta.Nifs.EphemeralStore do
  @moduledoc false

  alias Quanta.Nifs.Native

  @spec new(timeout_ms :: non_neg_integer()) :: {:ok, reference()}
  def new(timeout_ms \\ 30_000) when is_integer(timeout_ms) and timeout_ms >= 0 do
    Native.ephemeral_store_new(timeout_ms)
  end

  @spec set(store :: reference(), key :: String.t(), value :: binary()) :: :ok
  def set(store, key, value) when is_binary(key) and is_binary(value) do
    Native.ephemeral_store_set(store, key, value)
  end

  @spec get(store :: reference(), key :: String.t()) :: {:ok, binary()} | :not_found
  def get(store, key) when is_binary(key) do
    Native.ephemeral_store_get(store, key)
  end

  @spec delete(store :: reference(), key :: String.t()) :: :ok
  def delete(store, key) when is_binary(key) do
    Native.ephemeral_store_delete(store, key)
  end

  @spec get_all(store :: reference()) :: {:ok, %{String.t() => binary()}}
  def get_all(store) do
    Native.ephemeral_store_get_all(store)
  end

  @spec keys(store :: reference()) :: {:ok, [String.t()]}
  def keys(store) do
    Native.ephemeral_store_keys(store)
  end

  @spec encode(store :: reference(), key :: String.t()) :: {:ok, binary()}
  def encode(store, key) when is_binary(key) do
    Native.ephemeral_store_encode(store, key)
  end

  @spec encode_all(store :: reference()) :: {:ok, binary()}
  def encode_all(store) do
    Native.ephemeral_store_encode_all(store)
  end

  @spec apply_encoded(store :: reference(), bytes :: binary()) :: :ok | {:error, String.t()}
  def apply_encoded(store, bytes) when is_binary(bytes) do
    Native.ephemeral_store_apply_encoded(store, bytes)
  end
end
