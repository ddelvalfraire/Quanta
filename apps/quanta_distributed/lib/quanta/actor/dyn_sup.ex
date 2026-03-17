defmodule Quanta.Actor.DynSup do
  @moduledoc """
  Partitioned dynamic supervision for actors.

  Wraps N `DynamicSupervisor` instances behind a `PartitionSupervisor`,
  where N = `System.schedulers_online()`. Actor placement is deterministic
  via `:erlang.phash2(actor_id, N)`.
  """

  def child_spec(opts) do
    %{
      id: __MODULE__,
      start: {__MODULE__, :start_link, [opts]},
      type: :supervisor
    }
  end

  def start_link(_opts) do
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

  @spec start_actor(Quanta.ActorId.t(), keyword()) ::
          {:ok, pid()} | {:error, {:already_started, pid()} | term()}
  def start_actor(%Quanta.ActorId{} = actor_id, opts) do
    child_spec =
      Keyword.get_lazy(opts, :child_spec, fn ->
        {Quanta.Actor.Server, Keyword.put(opts, :actor_id, actor_id)}
      end)

    DynamicSupervisor.start_child(
      {:via, PartitionSupervisor, {__MODULE__, actor_id}},
      child_spec
    )
  end

  @spec stop_actor(pid()) :: :ok
  def stop_actor(pid) when is_pid(pid) do
    GenServer.stop(pid, :normal)
  end

  @spec count_actors() :: non_neg_integer()
  def count_actors do
    __MODULE__
    |> PartitionSupervisor.which_children()
    |> Enum.reduce(0, fn {_id, pid, _type, _modules}, acc ->
      %{active: active} = DynamicSupervisor.count_children(pid)
      acc + active
    end)
  end
end
