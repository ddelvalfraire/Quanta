defmodule Quanta.Web.TestSetup do
  @moduledoc false

  def reset_actor_environment do
    PartitionSupervisor.which_children(Quanta.Actor.DynSup)
    |> Enum.each(fn {_id, sup_pid, _type, _modules} ->
      DynamicSupervisor.which_children(sup_pid)
      |> Enum.each(fn {_, child_pid, _, _} ->
        DynamicSupervisor.terminate_child(sup_pid, child_pid)
      end)
    end)

    :sys.replace_state(Quanta.Actor.ManifestRegistry, fn state ->
      :ets.delete_all_objects(state)
      state
    end)

    if :ets.whereis(Quanta.RateLimit) != :undefined do
      :ets.delete(Quanta.RateLimit)
    end

    Quanta.RateLimit.init()

    Quanta.Actor.SchemaEvolution.reset_table()

    :ok =
      Quanta.Actor.ManifestRegistry.put(%Quanta.Manifest{
        version: "1",
        type: "counter",
        namespace: "test"
      })

    :ok =
      Quanta.Actor.ManifestRegistry.put(%Quanta.Manifest{
        version: "1",
        type: "crdt_doc",
        namespace: "test",
        state: %Quanta.Manifest.State{kind: {:crdt, :map}}
      })

    prev = Application.get_env(:quanta_distributed, :actor_modules, %{})

    Application.put_env(:quanta_distributed, :actor_modules, %{
      {"test", "counter"} => Quanta.Web.Test.Counter,
      {"test", "crdt_doc"} => Quanta.Web.Test.CrdtDoc
    })

    {:ok, prev_modules: prev}
  end
end
