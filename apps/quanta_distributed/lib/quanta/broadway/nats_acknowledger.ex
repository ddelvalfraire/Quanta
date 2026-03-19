defmodule Quanta.Broadway.NatsAcknowledger do
  @moduledoc """
  Broadway acknowledger for NATS JetStream messages.

  The Rust NIF auto-acks messages on fetch, so no per-message ack/nack
  back to JetStream is possible. This acknowledger emits telemetry for
  success/failure tracking only.
  """

  @behaviour Broadway.Acknowledger

  @impl true
  def ack(_ack_ref, successful, failed) do
    if successful != [] do
      :telemetry.execute(
        [:quanta, :broadway, :success],
        %{count: length(successful)},
        %{}
      )
    end

    if failed != [] do
      :telemetry.execute(
        [:quanta, :broadway, :failed],
        %{count: length(failed)},
        %{}
      )
    end

    :ok
  end
end
