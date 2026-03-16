defmodule Quanta.Effect do
  @moduledoc """
  Tagged-tuple types representing actor side effects.
  """

  @type t ::
          {:reply, binary()}
          | {:send, Quanta.ActorId.t(), binary()}
          | {:publish, String.t(), binary()}
          | {:persist, binary()}
          | {:set_timer, String.t(), pos_integer()}
          | {:cancel_timer, String.t()}
          | {:emit_telemetry, String.t(), map(), map()}
          | {:spawn_actor, Quanta.ActorId.t(), binary()}
          | :stop_self
          | {:side_effect, {module(), atom(), [term()]}}
end
