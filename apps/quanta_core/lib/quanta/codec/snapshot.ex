defmodule Quanta.Codec.Snapshot do
  @moduledoc """
  KV Snapshot binary format: fixed-width big-endian header + state data.

  Format: `<<js_seq::64, state_version::16, activation_nonce::64, state_data::binary>>`
  Header is exactly 18 bytes.
  """

  @header_size 18

  @type header :: %{
          js_seq: non_neg_integer(),
          state_version: non_neg_integer(),
          activation_nonce: non_neg_integer()
        }

  @spec header_size() :: 18
  def header_size, do: @header_size

  @spec encode(non_neg_integer(), non_neg_integer(), non_neg_integer(), binary()) :: binary()
  def encode(js_seq, state_version, activation_nonce, state_data)
      when is_integer(js_seq) and is_integer(state_version) and
             is_integer(activation_nonce) and is_binary(state_data) do
    <<js_seq::unsigned-big-64, state_version::unsigned-big-16,
      activation_nonce::unsigned-big-64, state_data::binary>>
  end

  @spec decode(binary()) :: {:ok, header(), binary()} | {:error, :invalid_snapshot}
  def decode(
        <<js_seq::unsigned-big-64, state_version::unsigned-big-16,
          activation_nonce::unsigned-big-64, state_data::binary>>
      ) do
    header = %{
      js_seq: js_seq,
      state_version: state_version,
      activation_nonce: activation_nonce
    }

    {:ok, header, state_data}
  end

  def decode(_), do: {:error, :invalid_snapshot}
end
