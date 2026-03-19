defmodule Quanta.Actor.SubscriberTracker do
  @spec any_subscribers?(Quanta.ActorId.t()) :: boolean()
  def any_subscribers?(actor_id) do
    case Application.get_env(:quanta_distributed, :presence_adapter) do
      nil ->
        false

      mod ->
        topic = "actor:#{actor_id.namespace}:#{actor_id.type}:#{actor_id.id}"
        mod.subscriber_count(topic) > 0
    end
  end
end
