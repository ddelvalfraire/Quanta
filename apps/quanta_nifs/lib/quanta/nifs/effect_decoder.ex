defmodule Quanta.Nifs.EffectDecoder do
  @moduledoc """
  Decodes effect maps returned by WASM actor calls into structured tuples.

  The NIF layer returns effects as maps with string keys (e.g.
  `%{"type" => "persist"}`). This module pattern-matches those maps
  into tagged tuples for easier use in Elixir.
  """

  @type effect ::
          :persist
          | {:send, target :: String.t(), payload :: binary(), correlation_id :: String.t() | nil}
          | {:reply, data :: binary()}
          | {:set_timer, name :: String.t(), delay_ms :: non_neg_integer()}
          | {:cancel_timer, name :: String.t()}
          | {:emit_event, data :: binary()}
          | {:log, message :: String.t()}

  @spec decode(map()) :: effect()
  def decode(%{"type" => "persist"}), do: :persist

  def decode(%{"type" => "send"} = effect) do
    {:send, effect["target"], effect["payload"], Map.get(effect, "correlation_id")}
  end

  def decode(%{"type" => "reply"} = effect) do
    {:reply, effect["data"]}
  end

  def decode(%{"type" => "set_timer"} = effect) do
    {:set_timer, effect["name"], effect["delay_ms"]}
  end

  def decode(%{"type" => "cancel_timer"} = effect) do
    {:cancel_timer, effect["name"]}
  end

  def decode(%{"type" => "emit_event"} = effect) do
    {:emit_event, effect["data"]}
  end

  def decode(%{"type" => "log"} = effect) do
    {:log, effect["message"]}
  end

  def decode(%{"type" => type} = effect) do
    raise ArgumentError, "unknown effect type #{inspect(type)}: #{inspect(effect)}"
  end

  @spec decode_all([map()]) :: [effect()]
  def decode_all(effects) when is_list(effects) do
    Enum.map(effects, &decode/1)
  end
end
