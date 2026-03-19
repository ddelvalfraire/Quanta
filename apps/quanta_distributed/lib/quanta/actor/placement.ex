defmodule Quanta.Actor.Placement do
  @moduledoc false

  alias Quanta.ActorId

  @spec target_node(ActorId.t()) :: node()
  def target_node(%ActorId{} = actor_id) do
    {:ok, ring} = Quanta.Cluster.Topology.ring()
    key = ActorId.to_placement_key(actor_id)
    {:ok, node} = ExHashRing.Ring.find_node(ring, key)
    node
  end

  @spec target_nodes(ActorId.t(), pos_integer()) :: [node()]
  def target_nodes(%ActorId{} = actor_id, count) do
    {:ok, ring} = Quanta.Cluster.Topology.ring()
    key = ActorId.to_placement_key(actor_id)
    {:ok, nodes} = ExHashRing.Ring.find_nodes(ring, key, count)
    nodes
  end

  @spec local?(ActorId.t()) :: boolean()
  def local?(%ActorId{} = actor_id) do
    target_node(actor_id) == node()
  end
end
