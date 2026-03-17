defmodule Quanta.Actor.Registry do
  @moduledoc """
  Distributed actor registry backed by Syn.

  Uses the `:actors` scope for cross-node pid lookup.
  """

  @scope :actors

  @doc false
  @spec scopes() :: [atom()]
  def scopes, do: [@scope]

  @spec lookup(Quanta.ActorId.t()) :: {:ok, pid()} | :not_found
  def lookup(%Quanta.ActorId{} = actor_id) do
    case :syn.lookup(@scope, actor_id) do
      {pid, _meta} -> {:ok, pid}
      :undefined -> :not_found
    end
  end

  @spec register(Quanta.ActorId.t()) :: :ok | {:error, :already_registered}
  def register(%Quanta.ActorId{} = actor_id) do
    meta = %{node: node(), type: actor_id.type}

    case :syn.register(@scope, actor_id, self(), meta) do
      :ok -> :ok
      {:error, :taken} -> {:error, :already_registered}
    end
  end

  @spec deregister(Quanta.ActorId.t()) :: :ok
  def deregister(%Quanta.ActorId{} = actor_id) do
    case :syn.unregister(@scope, actor_id) do
      :ok -> :ok
      {:error, :undefined} -> :ok
    end
  end
end
