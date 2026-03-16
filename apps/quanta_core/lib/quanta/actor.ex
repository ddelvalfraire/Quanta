defmodule Quanta.Actor do
  @moduledoc """
  Behaviour for actor implementations.

  State is `binary()` — actors run in a Wasm sandbox and the host sees opaque bytes.
  """

  @callback init(binary()) :: {binary(), [Quanta.Effect.t()]}

  @callback handle_message(binary(), Quanta.Envelope.t()) :: {binary(), [Quanta.Effect.t()]}

  @callback handle_timer(binary(), String.t()) :: {binary(), [Quanta.Effect.t()]}

  @callback on_passivate(binary()) :: binary()

  @optional_callbacks [on_passivate: 1]
end
