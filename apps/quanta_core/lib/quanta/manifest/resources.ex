defmodule Quanta.Manifest.Resources do
  @moduledoc false

  import Quanta.Manifest.Validation

  defstruct fuel_limit: 1_000_000,
            memory_limit_mb: 16,
            max_timers: 100

  @type t :: %__MODULE__{
          fuel_limit: pos_integer(),
          memory_limit_mb: pos_integer(),
          max_timers: pos_integer()
        }

  @fields [:fuel_limit, :memory_limit_mb, :max_timers]

  @doc false
  @spec from_map(map() | nil) :: t()
  def from_map(nil), do: %__MODULE__{}

  def from_map(map) when is_map(map) do
    defaults = %__MODULE__{}

    %__MODULE__{
      fuel_limit: default_or(map, "fuel_limit", defaults.fuel_limit),
      memory_limit_mb: default_or(map, "memory_limit_mb", defaults.memory_limit_mb),
      max_timers: default_or(map, "max_timers", defaults.max_timers)
    }
  end

  @spec validate(t()) :: [String.t()]
  def validate(%__MODULE__{} = r) do
    Enum.flat_map(@fields, fn field ->
      validate_positive_int(Map.fetch!(r, field), "resources.#{field}")
    end)
  end
end
