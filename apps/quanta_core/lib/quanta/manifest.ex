defmodule Quanta.Manifest do
  @moduledoc """
  Actor manifest: parsed from YAML, validated, and stored in the registry.
  """

  alias __MODULE__.{State, Lifecycle, Resources, RateLimits}

  @enforce_keys [:version, :type, :namespace]
  defstruct [
    :version,
    :type,
    :namespace,
    state: %State{},
    lifecycle: %Lifecycle{},
    resources: %Resources{},
    rate_limits: %RateLimits{}
  ]

  @type t :: %__MODULE__{
          version: String.t(),
          type: String.t(),
          namespace: String.t(),
          state: State.t(),
          lifecycle: Lifecycle.t(),
          resources: Resources.t(),
          rate_limits: RateLimits.t()
        }

  @segment_pattern ~r/^[a-zA-Z0-9_-]{1,63}$/
  @max_yaml_bytes 65_536

  @spec parse_yaml(String.t()) :: {:ok, t()} | {:error, [String.t()]}
  def parse_yaml(yaml) when byte_size(yaml) > @max_yaml_bytes do
    {:error, ["manifest YAML exceeds maximum size of #{@max_yaml_bytes} bytes"]}
  end

  def parse_yaml(yaml) do
    case YamlElixir.read_from_string(yaml) do
      {:ok, map} when is_map(map) ->
        manifest = from_map(map)

        case validate(manifest) do
          :ok -> {:ok, manifest}
          error -> error
        end

      {:ok, _} ->
        {:error, ["manifest must be a YAML mapping"]}

      {:error, %YamlElixir.ParsingError{message: msg}} ->
        {:error, ["YAML parse error: #{msg}"]}
    end
  end

  @spec validate(t()) :: :ok | {:error, [String.t()]}
  def validate(%__MODULE__{} = m) do
    errors =
      List.flatten([
        validate_version(m.version),
        validate_segment(m.type, "type"),
        validate_segment(m.namespace, "namespace"),
        State.validate(m.state),
        Lifecycle.validate(m.lifecycle),
        Resources.validate(m.resources),
        RateLimits.validate(m.rate_limits)
      ])

    case errors do
      [] -> :ok
      errs -> {:error, errs}
    end
  end

  @spec validate_update(old :: t(), new :: t()) :: :ok | {:error, String.t()}
  def validate_update(%__MODULE__{} = old, %__MODULE__{} = new) do
    cond do
      old.namespace != new.namespace ->
        {:error,
         "namespace is immutable (was #{inspect(old.namespace)}, got #{inspect(new.namespace)})"}

      old.type != new.type ->
        {:error, "type is immutable (was #{inspect(old.type)}, got #{inspect(new.type)})"}

      old.state.kind != new.state.kind ->
        {:error,
         "state.kind is immutable (was #{inspect(old.state.kind)}, got #{inspect(new.state.kind)})"}

      true ->
        :ok
    end
  end

  defp from_map(map) do
    %__MODULE__{
      version: map["version"],
      type: map["type"],
      namespace: map["namespace"],
      state: State.from_map(map["state"]),
      lifecycle: Lifecycle.from_map(map["lifecycle"]),
      resources: Resources.from_map(map["resources"]),
      rate_limits: RateLimits.from_map(map["rate_limits"])
    }
  end

  defp validate_version("1"), do: []
  defp validate_version(nil), do: ["version is required"]
  defp validate_version(v), do: ["version must be \"1\", got: #{inspect(v)}"]

  defp validate_segment(nil, name), do: ["#{name} is required"]

  defp validate_segment(val, name) when is_binary(val) do
    if Regex.match?(@segment_pattern, val) do
      []
    else
      ["#{name} must match ^[a-zA-Z0-9_-]{1,63}$, got: #{inspect(val)}"]
    end
  end

  defp validate_segment(val, name), do: ["#{name} must be a string, got: #{inspect(val)}"]
end
