defmodule Quanta.Actor.DynSup do
  @moduledoc """
  Partitioned dynamic supervision for actors.

  Wraps N `DynamicSupervisor` instances behind a `PartitionSupervisor`,
  where N = `System.schedulers_online()`. Actor placement is deterministic
  via `:erlang.phash2(actor_id, N)`.
  """

  @spec child_spec(any()) :: %{
          id: Quanta.Actor.DynSup,
          start: {Quanta.Actor.DynSup, :start_link, [...]},
          type: :supervisor
        }
  def child_spec(opts) do
    %{
      id: __MODULE__,
      start: {__MODULE__, :start_link, [opts]},
      type: :supervisor
    }
  end

  @counter_key :quanta_actor_counter

  @spec start_link(any()) :: :ignore | {:error, any()} | {:ok, pid()}
  def start_link(_opts) do
    case :persistent_term.get(@counter_key, nil) do
      nil -> :persistent_term.put(@counter_key, :atomics.new(1, signed: true))
      ref -> :atomics.put(ref, 1, 0)
    end

    PartitionSupervisor.start_link(
      child_spec:
        DynamicSupervisor.child_spec(
          strategy: :one_for_one,
          max_restarts: 10_000,
          max_seconds: 1
        ),
      name: __MODULE__,
      partitions: System.schedulers_online()
    )
  end

  @spec increment_count() :: :ok
  def increment_count do
    :atomics.add(:persistent_term.get(@counter_key), 1, 1)
    :ok
  end

  @spec decrement_count() :: :ok
  def decrement_count do
    :atomics.sub(:persistent_term.get(@counter_key), 1, 1)
    :ok
  end

  @spec start_actor(Quanta.ActorId.t(), keyword()) ::
          {:ok, pid()} | {:error, {:already_started, pid()} | term()}
  def start_actor(%Quanta.ActorId{} = actor_id, opts) do
    child_spec =
      Keyword.get_lazy(opts, :child_spec, fn ->
        {Quanta.Actor.Server, Keyword.put(opts, :actor_id, actor_id)}
      end)

    case DynamicSupervisor.start_child(
           {:via, PartitionSupervisor, {__MODULE__, actor_id}},
           child_spec
         ) do
      {:ok, pid} ->
        track_actor(pid)
        {:ok, pid}

      other ->
        other
    end
  end

  defp track_actor(pid) do
    ref = :persistent_term.get(@counter_key)
    :atomics.add(ref, 1, 1)

    spawn(fn ->
      mon = Process.monitor(pid)

      receive do
        {:DOWN, ^mon, :process, ^pid, _reason} ->
          :atomics.sub(ref, 1, 1)
      end
    end)
  end

  @spec stop_actor(pid()) :: :ok
  def stop_actor(pid) when is_pid(pid) do
    GenServer.stop(pid, :normal)
  end

  @doc "Fast actor count using atomic counter. May drift slightly on crashes."
  @spec count_actors_fast() :: non_neg_integer()
  def count_actors_fast do
    max(:atomics.get(:persistent_term.get(@counter_key), 1), 0)
  end

  @doc "Exact actor count by traversing all supervisor partitions. Slower but accurate."
  @spec count_actors() :: non_neg_integer()
  def count_actors do
    __MODULE__
    |> PartitionSupervisor.which_children()
    |> Enum.reduce(0, fn {_id, pid, _type, _modules}, acc ->
      %{active: active} = DynamicSupervisor.count_children(pid)
      acc + active
    end)
  end

  @doc "Returns a flat list of all actor pids across all partitions."
  @spec list_actor_pids() :: [pid()]
  def list_actor_pids do
    __MODULE__
    |> PartitionSupervisor.which_children()
    |> Enum.flat_map(fn {_id, sup_pid, _type, _modules} ->
      sup_pid
      |> DynamicSupervisor.which_children()
      |> Enum.reduce([], fn
        {_id, pid, _type, _modules}, acc when is_pid(pid) -> [pid | acc]
        _, acc -> acc
      end)
    end)
  end
end
