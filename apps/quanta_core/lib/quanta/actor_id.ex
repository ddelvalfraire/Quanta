defmodule Quanta.ActorId do
  @moduledoc """
  Composite actor identifier: `{namespace, type, id}`.

  Each segment must contain only alphanumeric characters, hyphens,
  underscores, and `@` (NATS-safe — no dots or wildcards).
  """

  @enforce_keys [:namespace, :type, :id]
  defstruct [:namespace, :type, :id]

  @type t :: %__MODULE__{
          namespace: String.t(),
          type: String.t(),
          id: String.t()
        }

  @segment_pattern ~r/^[a-zA-Z0-9_@-]+$/

  @spec validate(t()) :: :ok | {:error, String.t()}
  def validate(%__MODULE__{namespace: ns, type: type, id: id}) do
    with :ok <- validate_segment(ns, "namespace", 63),
         :ok <- validate_segment(type, "type", 63),
         :ok <- validate_segment(id, "id", 128) do
      :ok
    end
  end

  @spec to_nats_subject_fragment(t()) :: String.t()
  def to_nats_subject_fragment(%__MODULE__{namespace: ns, type: type, id: id}) do
    "#{ns}.#{type}.#{id}"
  end

  @doc "Returns a stable string key for consistent-hash placement."
  @spec to_placement_key(t()) :: String.t()
  def to_placement_key(%__MODULE__{namespace: ns, type: type, id: id}) do
    "#{ns}.#{type}.#{id}"
  end

  @doc "Identity — ActorId structs are used directly as syn registry keys."
  @spec to_syn_key(t()) :: t()
  def to_syn_key(%__MODULE__{} = actor_id), do: actor_id

  defp validate_segment(value, name, max_len) do
    cond do
      byte_size(value) == 0 ->
        {:error, "#{name} must not be empty"}

      byte_size(value) > max_len ->
        {:error, "#{name} must be at most #{max_len} characters"}

      not Regex.match?(@segment_pattern, value) ->
        {:error, "#{name} contains invalid characters (only a-zA-Z0-9_@- allowed)"}

      true ->
        :ok
    end
  end
end
