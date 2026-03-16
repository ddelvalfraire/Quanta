defmodule Quanta.Manifest.State do
  @moduledoc false

  alias Quanta.StateKind
  import Quanta.Manifest.Validation

  defstruct kind: :opaque,
            version: 1,
            max_size_bytes: 1_048_576,
            snapshot_interval: 100

  @type t :: %__MODULE__{
          kind: StateKind.t(),
          version: pos_integer(),
          max_size_bytes: pos_integer(),
          snapshot_interval: pos_integer()
        }

  @doc false
  @spec from_map(map() | nil) :: t()
  def from_map(nil), do: %__MODULE__{}

  def from_map(map) when is_map(map) do
    defaults = %__MODULE__{}

    kind =
      case map["kind"] do
        nil -> :opaque
        str when is_binary(str) -> parse_kind_or_raw(str)
        other -> {:raw, other}
      end

    %__MODULE__{
      kind: kind,
      version: default_or(map, "version", defaults.version),
      max_size_bytes: default_or(map, "max_size_bytes", defaults.max_size_bytes),
      snapshot_interval: default_or(map, "snapshot_interval", defaults.snapshot_interval)
    }
  end

  defp parse_kind_or_raw(str) do
    case StateKind.parse(str) do
      {:ok, kind} -> kind
      {:error, _} -> {:raw, str}
    end
  end

  @spec validate(t()) :: [String.t()]
  def validate(%__MODULE__{} = s) do
    List.flatten([
      validate_kind(s.kind),
      validate_int(s.version, "state.version", 1, 65_535),
      validate_int(s.max_size_bytes, "state.max_size_bytes", 1, 8_388_608),
      validate_positive_int(s.snapshot_interval, "state.snapshot_interval")
    ])
  end

  defp validate_kind({:raw, val}),
    do: ["invalid state.kind: #{inspect(val)}"]

  defp validate_kind(_), do: []
end
