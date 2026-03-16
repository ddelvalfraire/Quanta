defmodule Quanta.Manifest.Lifecycle do
  @moduledoc false

  import Quanta.Manifest.Validation

  defstruct idle_timeout_ms: 300_000,
            idle_no_subscribers_timeout_ms: 30_000,
            max_concurrent_messages: 1,
            inter_actor_timeout_ms: 30_000,
            http_timeout_ms: 5_000

  @type t :: %__MODULE__{
          idle_timeout_ms: pos_integer(),
          idle_no_subscribers_timeout_ms: pos_integer(),
          max_concurrent_messages: pos_integer(),
          inter_actor_timeout_ms: pos_integer(),
          http_timeout_ms: pos_integer()
        }

  @fields [
    :idle_timeout_ms,
    :idle_no_subscribers_timeout_ms,
    :max_concurrent_messages,
    :inter_actor_timeout_ms,
    :http_timeout_ms
  ]

  @doc false
  @spec from_map(map() | nil) :: t()
  def from_map(nil), do: %__MODULE__{}

  def from_map(map) when is_map(map) do
    defaults = %__MODULE__{}

    %__MODULE__{
      idle_timeout_ms: default_or(map, "idle_timeout_ms", defaults.idle_timeout_ms),
      idle_no_subscribers_timeout_ms:
        default_or(map, "idle_no_subscribers_timeout_ms", defaults.idle_no_subscribers_timeout_ms),
      max_concurrent_messages:
        default_or(map, "max_concurrent_messages", defaults.max_concurrent_messages),
      inter_actor_timeout_ms:
        default_or(map, "inter_actor_timeout_ms", defaults.inter_actor_timeout_ms),
      http_timeout_ms: default_or(map, "http_timeout_ms", defaults.http_timeout_ms)
    }
  end

  @spec validate(t()) :: [String.t()]
  def validate(%__MODULE__{} = l) do
    Enum.flat_map(@fields, fn field ->
      validate_positive_int(Map.fetch!(l, field), "lifecycle.#{field}")
    end)
  end
end
