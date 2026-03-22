defmodule Quanta.Codec.Bridge do
  @moduledoc """
  Bridge protocol codec — thin wrapper over the bridge NIF.

  Encodes/decodes bridge envelopes for communication between
  the realtime server (Rust) and the Elixir runtime.
  """

  alias Quanta.Nifs.Native

  @type msg_type ::
          :activate_island
          | :deactivate_island
          | :player_join
          | :player_leave
          | :entity_command
          | :state_sync
          | :heartbeat
          | :capacity_report

  @type header :: %{
          msg_type: msg_type(),
          sequence: non_neg_integer(),
          timestamp: non_neg_integer(),
          correlation_id: <<_::128>> | nil
        }

  @doc "Encode a bridge header + payload into a wire frame binary."
  @spec encode(header(), binary()) :: {:ok, binary()} | {:error, String.t()}
  def encode(header, payload) when is_map(header) and is_binary(payload) do
    Native.encode_bridge_envelope(header, payload)
  end

  @doc "Decode a wire frame binary into {header, payload}."
  @spec decode(binary()) :: {:ok, header(), binary()} | {:error, String.t()}
  def decode(frame) when is_binary(frame) do
    Native.decode_bridge_envelope(frame)
  end
end
