defmodule Quanta.Manifest.RateLimits do
  @moduledoc false

  import Quanta.Manifest.Validation

  defstruct messages_per_second: 1_000,
            messages_per_second_type: 100_000

  @type t :: %__MODULE__{
          messages_per_second: pos_integer(),
          messages_per_second_type: pos_integer()
        }

  @fields [:messages_per_second, :messages_per_second_type]

  @doc false
  @spec from_map(map() | nil) :: t()
  def from_map(nil), do: %__MODULE__{}

  def from_map(map) when is_map(map) do
    defaults = %__MODULE__{}

    %__MODULE__{
      messages_per_second: default_or(map, "messages_per_second", defaults.messages_per_second),
      messages_per_second_type:
        default_or(map, "messages_per_second_type", defaults.messages_per_second_type)
    }
  end

  @spec validate(t()) :: [String.t()]
  def validate(%__MODULE__{} = r) do
    Enum.flat_map(@fields, fn field ->
      validate_positive_int(Map.fetch!(r, field), "rate_limits.#{field}")
    end)
  end
end
