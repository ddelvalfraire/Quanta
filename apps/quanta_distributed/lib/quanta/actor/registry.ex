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

  @spec register(Quanta.ActorId.t(), map()) :: :ok | {:error, :already_registered}
  def register(%Quanta.ActorId{} = actor_id, extra_meta \\ %{}) do
    meta =
      %{
        node: node(),
        type: actor_id.type,
        nonce: :rand.uniform(0xFFFFFFFFFFFFFFFF),
        activated_at: System.monotonic_time(),
        draining: false
      }
      |> Map.merge(extra_meta)

    case :syn.register(@scope, actor_id, self(), meta) do
      :ok -> :ok
      {:error, :taken} -> {:error, :already_registered}
    end
  end

  @doc """
  Updates metadata for the calling process's registration of `actor_id`.

  `fun` receives the current metadata map and must return the new metadata map.
  """
  @spec update_meta(Quanta.ActorId.t(), (map() -> map())) ::
          {:ok, {pid(), map()}} | {:error, term()}
  def update_meta(%Quanta.ActorId{} = actor_id, fun) when is_function(fun, 1) do
    :syn.update_registry(@scope, actor_id, fn _pid, meta -> fun.(meta) end)
  end

  @doc "Returns the number of actors registered on the local node."
  @spec local_count() :: non_neg_integer()
  def local_count, do: :syn.registry_count(@scope, node())

  @doc "Returns the total number of actors registered across the cluster."
  @spec cluster_count() :: non_neg_integer()
  def cluster_count, do: :syn.registry_count(@scope)

  @doc "Marks a local actor as draining in its registry metadata."
  @spec mark_draining(Quanta.ActorId.t()) :: {:ok, {pid(), map()}} | {:error, term()}
  def mark_draining(%Quanta.ActorId{} = actor_id) do
    update_meta(actor_id, &Map.put(&1, :draining, true))
  end

  @doc "Returns all `{actor_id, pid, meta}` tuples registered on the local node."
  @spec local_actor_ids() :: [{Quanta.ActorId.t(), pid(), map()}]
  def local_actor_ids do
    Quanta.Actor.DynSup.list_actor_pids()
    |> Enum.reduce([], fn pid, acc ->
      try do
        %{actor_id: actor_id} = :sys.get_state(pid, 3_000)
        {_pid, meta} = :syn.lookup(@scope, actor_id)
        [{actor_id, pid, meta} | acc]
      catch
        :exit, _ -> acc
      end
    end)
  end

  @spec deregister(Quanta.ActorId.t()) :: :ok
  def deregister(%Quanta.ActorId{} = actor_id) do
    case :syn.unregister(@scope, actor_id) do
      :ok -> :ok
      {:error, :undefined} -> :ok
    end
  end
end
