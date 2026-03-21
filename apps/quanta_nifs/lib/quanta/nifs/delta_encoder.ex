defmodule Quanta.Nifs.DeltaEncoder do
  @moduledoc false

  alias Quanta.Nifs.Native

  @spec compute_delta(reference(), binary(), binary(), binary() | nil) ::
          {:ok, binary()} | {:error, term()}
  def compute_delta(schema, old_state, new_state, field_group_mask \\ nil)
      when is_reference(schema) and is_binary(old_state) and is_binary(new_state) do
    Native.delta_compute(schema, old_state, new_state, field_group_mask)
  end

  @spec apply_delta(reference(), binary(), binary()) ::
          {:ok, binary()} | {:error, term()}
  def apply_delta(schema, current_state, delta)
      when is_reference(schema) and is_binary(current_state) and is_binary(delta) do
    Native.delta_apply(schema, current_state, delta)
  end

  @spec decode_state(reference(), binary()) ::
          {:ok, map()} | {:error, term()}
  def decode_state(schema, state_bytes)
      when is_reference(schema) and is_binary(state_bytes) do
    Native.delta_decode_state(schema, state_bytes)
  end

  @spec encode_state(reference(), [term()]) ::
          {:ok, binary()} | {:error, term()}
  def encode_state(schema, values)
      when is_reference(schema) and is_list(values) do
    Native.delta_encode_state(schema, values)
  end
end
