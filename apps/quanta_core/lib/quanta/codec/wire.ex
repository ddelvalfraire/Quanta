defmodule Quanta.Codec.Wire do
  @moduledoc """
  NATS wire format: version byte + length-prefixed header + payload.

  Format: `<<0x01, header_size::32, header::binary-size(header_size), payload::binary>>`

  The header is currently encoded via `:erlang.term_to_binary` as an interim
  until the bitcode NIF (T05) is available. The public API is stable — only
  the internal encoding changes.
  """

  @wire_version 0x01

  @spec wire_version() :: 0x01
  def wire_version, do: @wire_version

  @spec encode(Quanta.Envelope.t()) :: binary()
  def encode(%Quanta.Envelope{} = envelope) do
    header_bytes = encode_header(envelope)
    header_size = byte_size(header_bytes)

    <<@wire_version::8, header_size::unsigned-big-32, header_bytes::binary,
      envelope.payload::binary>>
  end

  @spec decode(binary()) ::
          {:ok, Quanta.Envelope.t()} | {:error, :invalid_wire_format | :unsupported_wire_version}
  def decode(<<@wire_version::8, header_size::unsigned-big-32, rest::binary>>)
      when byte_size(rest) >= header_size do
    <<header_bytes::binary-size(header_size), payload::binary>> = rest

    case decode_header(header_bytes) do
      {:ok, header} ->
        envelope = %Quanta.Envelope{
          message_id: header.message_id,
          timestamp: %Quanta.HLC{wall: header.wall_us, logical: header.logical},
          correlation_id: header.correlation_id,
          causation_id: header.causation_id,
          sender: decode_sender(header.sender),
          payload: payload,
          metadata: header.metadata
        }

        {:ok, envelope}

      {:error, _} = err ->
        err
    end
  end

  def decode(<<version::8, _::binary>>) when version != @wire_version do
    {:error, :unsupported_wire_version}
  end

  def decode(_), do: {:error, :invalid_wire_format}

  defp encode_header(%Quanta.Envelope{} = envelope) do
    header = %{
      message_id: envelope.message_id,
      wall_us: envelope.timestamp.wall,
      logical: envelope.timestamp.logical,
      correlation_id: envelope.correlation_id,
      causation_id: envelope.causation_id,
      sender: encode_sender(envelope.sender),
      metadata: envelope.metadata
    }

    :erlang.term_to_binary(header)
  end

  defp decode_header(binary) do
    case safe_binary_to_term(binary) do
      {:ok,
       %{
         message_id: mid,
         wall_us: wall,
         logical: logical,
         correlation_id: _,
         causation_id: _,
         sender: _,
         metadata: _
       } = header}
      when is_binary(mid) and is_integer(wall) and is_integer(logical) ->
        {:ok, header}

      _ ->
        {:error, :invalid_header}
    end
  end

  defp encode_sender(%Quanta.ActorId{namespace: ns, type: type, id: id}),
    do: {:actor, ns, type, id}

  defp encode_sender({:client, id}), do: {:client, id}
  defp encode_sender(:system), do: :system
  defp encode_sender(nil), do: nil

  defp decode_sender({:actor, ns, type, id}),
    do: %Quanta.ActorId{namespace: ns, type: type, id: id}

  defp decode_sender({:client, id}), do: {:client, id}
  defp decode_sender(:system), do: :system
  defp decode_sender(nil), do: nil
  defp decode_sender(_), do: nil

  defp safe_binary_to_term(binary) do
    {:ok, :erlang.binary_to_term(binary, [:safe])}
  rescue
    ArgumentError -> :error
  end
end
