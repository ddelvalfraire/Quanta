defmodule Quanta.Manifest.Validation do
  @moduledoc false

  @spec validate_positive_int(term(), String.t()) :: [String.t()]
  def validate_positive_int(val, _name) when is_integer(val) and val > 0, do: []

  def validate_positive_int(val, name) when is_integer(val),
    do: ["#{name} must be positive, got: #{val}"]

  def validate_positive_int(val, name),
    do: ["#{name} must be a positive integer, got: #{inspect(val)}"]

  @spec validate_int(term(), String.t(), integer(), integer()) :: [String.t()]
  def validate_int(val, name, min, max) when is_integer(val) do
    if val >= min and val <= max do
      []
    else
      ["#{name} must be between #{min} and #{max}, got: #{val}"]
    end
  end

  def validate_int(val, name, _min, _max),
    do: ["#{name} must be an integer, got: #{inspect(val)}"]

  @doc false
  def default_or(map, key, default) when is_map(map) do
    case Map.get(map, key) do
      nil -> default
      val -> val
    end
  end
end
