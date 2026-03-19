defmodule Quanta.Web.ChannelCase do
  @moduledoc false
  use ExUnit.CaseTemplate

  using do
    quote do
      import Phoenix.ChannelTest

      @endpoint Quanta.Web.Endpoint

      @admin_key "qk_admin_test_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
      @rw_key "qk_rw_test_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
      @ro_key "qk_ro_test_cccccccccccccccccccccccccccccccc"
    end
  end

  setup do
    # Clear DynSup children
    PartitionSupervisor.which_children(Quanta.Actor.DynSup)
    |> Enum.each(fn {_id, sup_pid, _type, _modules} ->
      DynamicSupervisor.which_children(sup_pid)
      |> Enum.each(fn {_, child_pid, _, _} ->
        DynamicSupervisor.terminate_child(sup_pid, child_pid)
      end)
    end)

    # Reset ManifestRegistry ETS
    :sys.replace_state(Quanta.Actor.ManifestRegistry, fn state ->
      :ets.delete_all_objects(state)
      state
    end)

    # Ensure clean RateLimit ETS
    if :ets.whereis(Quanta.RateLimit) != :undefined do
      :ets.delete(Quanta.RateLimit)
    end

    Quanta.RateLimit.init()

    # Seed test manifest
    :ok =
      Quanta.Actor.ManifestRegistry.put(%Quanta.Manifest{
        version: "1",
        type: "counter",
        namespace: "test"
      })

    # Configure actor modules
    prev = Application.get_env(:quanta_distributed, :actor_modules, %{})

    Application.put_env(:quanta_distributed, :actor_modules, %{
      {"test", "counter"} => Quanta.Web.Test.Counter
    })

    on_exit(fn ->
      Application.put_env(:quanta_distributed, :actor_modules, prev)
    end)

    :ok
  end
end
