defmodule Quanta.Nats.JetStream.Behaviour do
  @moduledoc """
  Callback definitions for the JetStream API.

  Enables module-swap testing via application env without adding Mox.
  """

  @callback publish(String.t(), binary(), non_neg_integer() | nil) ::
              {:ok, %{stream: String.t(), seq: non_neg_integer()}}
              | {:error, term()}

  @callback kv_get(String.t(), String.t()) ::
              {:ok, binary(), non_neg_integer()} | {:error, term()}

  @callback kv_put(String.t(), String.t(), binary()) ::
              {:ok, non_neg_integer()} | {:error, term()}

  @callback kv_delete(String.t(), String.t()) :: :ok | {:error, term()}

  @callback consumer_create(String.t(), String.t(), non_neg_integer()) ::
              {:ok, reference()} | {:error, term()}

  @callback consumer_fetch(reference(), pos_integer(), pos_integer()) ::
              {:ok, list(map())} | {:error, term()}

  @callback consumer_delete(reference()) :: :ok | {:error, term()}

  @callback purge_subject(String.t(), String.t()) :: :ok | {:error, term()}
end
