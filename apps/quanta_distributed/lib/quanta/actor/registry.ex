defmodule Quanta.Actor.Registry do
  @moduledoc """
  Distributed actor registry backed by Syn.

  Uses the `:actors` scope for cross-node pid lookup.

  ## Local index

  Syn does not expose a public API for listing all names registered on the
  local node — its `:syn.lookup/2` is name-addressed, and the only way to
  iterate previously was via `syn_backbone.get_table_name/2` (private), an
  undocumented internal that leaks syn's private ETS layout and silently
  breaks on version bumps.

  To stay on public API while still supporting the drain hot path
  (`Quanta.Drain`), we maintain a tiny named ETS table,
  `:quanta_registry_local_index`, mirroring `{actor_id => pid}` for
  locally registered actors. The mirror is kept in sync by
  `Quanta.Actor.SynEventHandler`, which receives every
  `on_process_registered/5` and `on_process_unregistered/5` callback from
  syn. Meta is intentionally not cached — we re-fetch it with
  `:syn.lookup/2` so it always reflects current syn state (including
  `:draining` flips from `update_meta/2`).
  """

  @scope :actors
  @local_index_table :quanta_registry_local_index

  @doc false
  @spec scopes() :: [atom()]
  def scopes, do: [@scope]

  @doc """
  Initialises the local-node index ETS table. Idempotent; safe to call
  from supervision tree startup and from test setups.
  """
  @spec init_local_index() :: :ok
  def init_local_index do
    case :ets.info(@local_index_table) do
      :undefined ->
        :ets.new(@local_index_table, [
          :set,
          :public,
          :named_table,
          {:read_concurrency, true},
          {:write_concurrency, true}
        ])

        :ok

      _ ->
        :ok
    end
  end

  @doc false
  @spec local_index_table() :: atom()
  def local_index_table, do: @local_index_table

  @doc false
  @spec track_local(Quanta.ActorId.t(), pid()) :: :ok
  def track_local(%Quanta.ActorId{} = actor_id, pid) when is_pid(pid) do
    if node(pid) == node() and :ets.info(@local_index_table) != :undefined do
      :ets.insert(@local_index_table, {actor_id, pid})
    end

    :ok
  end

  @doc false
  @spec untrack_local(Quanta.ActorId.t()) :: :ok
  def untrack_local(%Quanta.ActorId{} = actor_id) do
    if :ets.info(@local_index_table) != :undefined do
      :ets.delete(@local_index_table, actor_id)
    end

    :ok
  end

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
      :ok ->
        # Defence-in-depth: also track directly on the caller side. The syn
        # event handler will insert the same row on `on_process_registered`,
        # but that callback is async — making it explicit here guarantees
        # `local_actor_ids/0` sees the registration immediately after the
        # call returns (matching syn's synchronous `register` semantics).
        track_local(actor_id, self())
        :ok

      {:error, :taken} ->
        {:error, :already_registered}
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

  @doc """
  Returns all `{actor_id, pid, meta}` tuples registered on the local node.

  Reads `actor_id` + `pid` from the local-node mirror, then fetches fresh
  meta from syn via `:syn.lookup/2`. Stale entries (where syn has already
  unregistered the name but the mirror has not caught up) are skipped —
  the mirror is eventually consistent via `SynEventHandler` callbacks.
  """
  @spec local_actor_ids() :: [{Quanta.ActorId.t(), pid(), map()}]
  def local_actor_ids do
    case :ets.info(@local_index_table) do
      :undefined ->
        []

      _ ->
        @local_index_table
        |> :ets.tab2list()
        |> Enum.flat_map(fn {actor_id, _pid} ->
          case :syn.lookup(@scope, actor_id) do
            {pid, meta} when is_pid(pid) -> [{actor_id, pid, meta}]
            :undefined -> []
          end
        end)
    end
  end

  @spec deregister(Quanta.ActorId.t()) :: :ok
  def deregister(%Quanta.ActorId{} = actor_id) do
    untrack_local(actor_id)

    case :syn.unregister(@scope, actor_id) do
      :ok -> :ok
      {:error, :undefined} -> :ok
    end
  end
end
