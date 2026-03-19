defmodule Quanta.Codec.WirePropertyTest do
  @moduledoc """
  Property test P5: Wire codec roundtrip.

  `decode(encode(envelope)) == envelope` for randomly generated envelopes.
  """

  use ExUnit.Case, async: true
  use PropCheck

  alias Quanta.Codec.Wire
  alias Quanta.Envelope
  alias Quanta.HLC
  alias Quanta.ActorId

  @moduletag :property

  # ── Generators ──────────────────────────────────────────────────────

  @segment_chars ~c"abcdefghijklmnopqrstuvwxyz0123456789_-"

  defp segment_gen do
    let chars <- non_empty(list(oneof(@segment_chars))) do
      List.to_string(chars)
    end
  end

  defp ulid_gen do
    # Generate a 26-char ULID-like string (alphanumeric)
    let chars <- vector(26, oneof(~c"0123456789ABCDEFGHJKMNPQRSTVWXYZ")) do
      List.to_string(chars)
    end
  end

  defp hlc_gen do
    let {wall, logical} <- {non_neg_integer(), integer(0, 65535)} do
      %HLC{wall: wall, logical: logical}
    end
  end

  defp actor_id_gen do
    let {ns, type, id} <- {segment_gen(), segment_gen(), segment_gen()} do
      %ActorId{namespace: ns, type: type, id: id}
    end
  end

  defp sender_gen do
    oneof([
      let(aid <- actor_id_gen(), do: aid),
      let(id <- segment_gen(), do: {:client, id}),
      :system,
      nil
    ])
  end

  defp nullable_string_gen do
    oneof([nil, ulid_gen()])
  end

  defp metadata_gen do
    let pairs <- list({segment_gen(), segment_gen()}) do
      Map.new(pairs)
    end
  end

  defp payload_gen do
    let bytes <- list(byte()) do
      :erlang.list_to_binary(bytes)
    end
  end

  defp envelope_gen do
    let {mid, ts, corr, cause, sender, payload, meta} <-
          {ulid_gen(), hlc_gen(), nullable_string_gen(), nullable_string_gen(),
           sender_gen(), payload_gen(), metadata_gen()} do
      %Envelope{
        message_id: mid,
        timestamp: ts,
        correlation_id: corr,
        causation_id: cause,
        sender: sender,
        payload: payload,
        metadata: meta
      }
    end
  end

  # ── Properties ──────────────────────────────────────────────────────

  property "roundtrip: decode(encode(envelope)) == {:ok, envelope}" do
    forall envelope <- envelope_gen() do
      wire = Wire.encode(envelope)
      {:ok, decoded} = Wire.decode(wire)

      decoded.message_id == envelope.message_id and
        decoded.timestamp == envelope.timestamp and
        decoded.correlation_id == envelope.correlation_id and
        decoded.causation_id == envelope.causation_id and
        decoded.sender == envelope.sender and
        decoded.payload == envelope.payload and
        decoded.metadata == envelope.metadata
    end
  end

  property "encode always produces binary starting with version byte 0x01" do
    forall envelope <- envelope_gen() do
      <<version::8, _rest::binary>> = Wire.encode(envelope)
      version == 0x01
    end
  end

  property "decode rejects wrong version" do
    forall {envelope, bad_version} <- {envelope_gen(), integer(2, 255)} do
      <<_::8, rest::binary>> = Wire.encode(envelope)
      bad_wire = <<bad_version::8, rest::binary>>
      Wire.decode(bad_wire) == {:error, :unsupported_wire_version}
    end
  end

  property "decode rejects truncated input" do
    forall envelope <- envelope_gen() do
      wire = Wire.encode(envelope)

      if byte_size(wire) > 5 do
        # Truncate to just version + partial header size
        truncated = binary_part(wire, 0, 3)
        Wire.decode(truncated) == {:error, :invalid_wire_format}
      else
        true
      end
    end
  end
end
