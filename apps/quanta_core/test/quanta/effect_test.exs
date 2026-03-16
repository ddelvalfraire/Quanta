defmodule Quanta.EffectTest do
  use ExUnit.Case, async: true

  test "all effect variants are constructable" do
    actor_id = %Quanta.ActorId{namespace: "ns", type: "t", id: "1"}

    effects = [
      {:reply, <<>>},
      {:send, actor_id, <<>>},
      {:publish, "topic", <<>>},
      {:persist, <<>>},
      {:set_timer, "check", 5000},
      {:cancel_timer, "check"},
      {:emit_telemetry, "event", %{}, %{}},
      {:spawn_actor, actor_id, <<>>},
      :stop_self,
      {:side_effect, {IO, :puts, ["hello"]}}
    ]

    assert length(effects) == 10
  end
end
