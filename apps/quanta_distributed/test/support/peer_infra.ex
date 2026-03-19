defmodule Quanta.Test.PeerInfra do
  @moduledoc false

  @doc false
  def boot(opts \\ []) do
    test_pid = self()

    holder =
      spawn(fn ->
        Process.flag(:trap_exit, true)
        Process.register(self(), __MODULE__)

        manifests = Keyword.get(opts, :manifests, [])
        actor_modules = Keyword.get(opts, :actor_modules, %{})

        Quanta.RateLimit.init()

        {:ok, _} = Quanta.Actor.ManifestRegistry.start_link([])
        for m <- manifests, do: :ok = Quanta.Actor.ManifestRegistry.put(m)

        Application.put_env(:quanta_distributed, :actor_modules, actor_modules)

        if :ets.whereis(:quanta_actor_init_attempts) == :undefined do
          :ets.new(:quanta_actor_init_attempts, [:named_table, :public, :set])
        end

        {:ok, _} = Quanta.Actor.DynSup.start_link([])

        # Topology is intentionally omitted — RPC targets always activate
        # locally, preventing multi-hop bouncing across nodes.

        send(test_pid, {__MODULE__, :ready})
        receive do: (:stop -> :ok)
      end)

    receive do
      {__MODULE__, :ready} -> {:ok, holder}
    after
      5_000 -> {:error, :timeout}
    end
  end
end
