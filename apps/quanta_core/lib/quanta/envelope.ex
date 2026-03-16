defmodule Quanta.Envelope do
  @moduledoc """
  Message wrapper carrying payload, identity, and causal metadata.

  `new/1` auto-generates a ULID message ID and an HLC timestamp.
  """

  @enforce_keys [:message_id, :timestamp, :payload]
  defstruct [
    :message_id,
    :timestamp,
    :correlation_id,
    :causation_id,
    :sender,
    :payload,
    metadata: %{}
  ]

  @type t :: %__MODULE__{
          message_id: String.t(),
          timestamp: Quanta.HLC.t(),
          correlation_id: String.t() | nil,
          causation_id: String.t() | nil,
          sender: Quanta.ActorId.t() | {:client, String.t()} | :system | nil,
          payload: binary(),
          metadata: %{String.t() => String.t()}
        }

  @spec new(keyword()) :: t()
  def new(opts) when is_list(opts) do
    struct!(
      __MODULE__,
      opts
      |> Keyword.put_new_lazy(:message_id, &Quanta.ULID.generate/0)
      |> Keyword.put_new_lazy(:timestamp, &Quanta.HLC.now/0)
    )
  end
end
