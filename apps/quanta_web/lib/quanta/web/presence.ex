defmodule Quanta.Web.Presence do
  use Phoenix.Presence,
    otp_app: :quanta_web,
    pubsub_server: Quanta.Web.PubSub

  @spec subscriber_count(String.t()) :: non_neg_integer()
  def subscriber_count(topic) do
    topic |> list() |> map_size()
  end
end
