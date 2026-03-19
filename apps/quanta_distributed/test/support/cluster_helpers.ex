defmodule Quanta.Test.ClusterHelpers do
  @moduledoc """
  LocalCluster helpers for multi-node tests.

  Boots a cluster of BEAM nodes with the Quanta app stack (sans PropCheck/Web),
  registers manifests, and waits for the hash ring to sync.
  """

  @counter_manifest %Quanta.Manifest{
    version: "1",
    namespace: "test",
    type: "counter",
    state: %Quanta.Manifest.State{},
    lifecycle: %Quanta.Manifest.Lifecycle{},
    resources: %Quanta.Manifest.Resources{},
    rate_limits: %Quanta.Manifest.RateLimits{}
  }

  # Apps to start on cluster nodes — explicitly listed to exclude propcheck
  # (which is in the .app file for test env but crashes without a .ctex file)
  # and quanta_web (which needs Phoenix/Cowboy ports).
  @cluster_apps [
    :crypto, :telemetry, :ex_hash_ring, :syn, :gnat, :jason,
    :yaml_elixir, :rustler, :libcluster, :broadway, :gen_stage,
    :nimble_options, :quanta_core, :quanta_nifs, :quanta_distributed
  ]

  @doc """
  Start a LocalCluster with `count` nodes, configured for Quanta.

  Returns `{:ok, cluster_pid, nodes}`.
  """
  @spec start_cluster(String.t(), pos_integer(), keyword()) ::
          {:ok, pid(), [node()]}
  def start_cluster(prefix, count, opts \\ []) do
    manifests = Keyword.get(opts, :manifests, [@counter_manifest])
    actor_modules = Keyword.get(opts, :actor_modules, %{{"test", "counter"} => Quanta.Test.Actors.Counter})

    LocalCluster.start()

    {:ok, cluster} =
      LocalCluster.start_link(count,
        prefix: prefix,
        applications: @cluster_apps,
        environment: [
          quanta_distributed: [
            actor_modules: actor_modules,
            presence_adapter: nil,
            nats_urls: ["nats://127.0.0.1:19222"]
          ]
        ]
      )

    {:ok, nodes} = LocalCluster.nodes(cluster)

    # Wait for ManifestRegistry to be ready, then register manifests
    for node <- nodes do
      wait_for_ready(node)

      for manifest <- manifests do
        :ok = :rpc.call(node, Quanta.Actor.ManifestRegistry, :put, [manifest], 10_000)
      end
    end

    # Also register manifests + actor_modules on the manager node —
    # the hash ring includes it, so it may receive routed actors.
    Application.put_env(:quanta_distributed, :actor_modules, actor_modules)

    for manifest <- manifests do
      :ok = Quanta.Actor.ManifestRegistry.put(manifest)
    end

    # Wait for hash ring to discover all nodes
    wait_for_topology(nodes)

    {:ok, cluster, nodes}
  end

  defp wait_for_ready(node) do
    wait_until(15_000, fn ->
      case :rpc.call(node, Process, :whereis, [Quanta.Actor.ManifestRegistry]) do
        pid when is_pid(pid) -> true
        _ -> false
      end
    end)
  end

  @doc "Wait until all cluster nodes are in each other's hash rings."
  @spec wait_for_topology([node()]) :: :ok
  def wait_for_topology(nodes) do
    all_nodes = MapSet.new([node() | nodes])

    wait_until(10_000, fn ->
      Enum.all?(nodes, fn n ->
        case :rpc.call(n, Quanta.Cluster.Topology, :nodes, []) do
          {:badrpc, _} -> false
          ring_nodes -> MapSet.subset?(all_nodes, MapSet.new(ring_nodes))
        end
      end)
    end)
  end

  @doc "Stop the cluster."
  @spec stop_cluster(pid()) :: :ok
  def stop_cluster(cluster) do
    LocalCluster.stop(cluster)
    :ok
  end

  @doc "Lookup an actor across the cluster via Syn."
  @spec cluster_lookup(node(), Quanta.ActorId.t()) :: {:ok, pid()} | :not_found
  def cluster_lookup(node, actor_id) do
    :rpc.call(node, Quanta.Actor.Registry, :lookup, [actor_id])
  end

  @doc "Count local actors on a given node."
  @spec local_count(node()) :: non_neg_integer()
  def local_count(node) do
    :rpc.call(node, Quanta.Actor.Registry, :local_count, [])
  end

  @doc "Route a message via CommandRouter on a specific node."
  @spec route_on(node(), Quanta.ActorId.t(), Quanta.Envelope.t(), pos_integer()) :: term()
  def route_on(node, actor_id, envelope, timeout \\ 30_000) do
    :rpc.call(node, Quanta.Actor.CommandRouter, :route, [actor_id, envelope, timeout])
  end

  @doc "Stop a specific node within a cluster."
  @spec stop_node(pid(), node()) :: :ok
  def stop_node(cluster, node) do
    LocalCluster.stop(cluster, node)
    :ok
  end

  defp wait_until(timeout, fun) do
    deadline = System.monotonic_time(:millisecond) + timeout
    do_wait(deadline, fun)
  end

  defp do_wait(deadline, fun) do
    if fun.() do
      :ok
    else
      if System.monotonic_time(:millisecond) >= deadline do
        raise "Timed out waiting for cluster sync"
      end

      Process.sleep(100)
      do_wait(deadline, fun)
    end
  end
end
