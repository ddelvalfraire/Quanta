defmodule Quanta.ULID do
  @moduledoc """
  Monotonic ULID generation with Crockford base32 encoding.

  Each ULID is a 26-character string: 10 chars of timestamp (48-bit ms)
  followed by 16 chars of randomness (80-bit). Sequential calls within
  the same millisecond produce monotonically increasing values by
  incrementing the random component.

  Monotonicity is per-process (uses the process dictionary). This is
  sufficient for the actor runtime where each actor runs in its own process.
  """

  import Bitwise

  @crockford "0123456789ABCDEFGHJKMNPQRSTVWXYZ"

  @spec generate() :: String.t()
  def generate do
    generate(System.system_time(:millisecond))
  end

  @spec generate(integer()) :: String.t()
  def generate(timestamp_ms) when is_integer(timestamp_ms) do
    <<rand_hi::40, rand_lo::40>> = monotonic_random(timestamp_ms)

    encode_crockford(timestamp_ms, 10) <>
      encode_crockford(rand_hi, 8) <>
      encode_crockford(rand_lo, 8)
  end

  defp monotonic_random(timestamp_ms) do
    case Process.get({__MODULE__, :state}) do
      {^timestamp_ms, <<prev_hi::40, prev_lo::40>>} ->
        {new_hi, new_lo} =
          cond do
            prev_lo < 0xFFFFFFFFFF -> {prev_hi, prev_lo + 1}
            prev_hi < 0xFFFFFFFFFF -> {prev_hi + 1, 0}
            true -> raise "ULID random component overflow within same millisecond"
          end

        random = <<new_hi::40, new_lo::40>>
        Process.put({__MODULE__, :state}, {timestamp_ms, random})
        random

      _ ->
        random = :crypto.strong_rand_bytes(10)
        Process.put({__MODULE__, :state}, {timestamp_ms, random})
        random
    end
  end

  defp encode_crockford(value, length) do
    for i <- (length - 1)..0//-1, into: <<>> do
      <<:binary.at(@crockford, value >>> (i * 5) &&& 0x1F)>>
    end
  end
end
