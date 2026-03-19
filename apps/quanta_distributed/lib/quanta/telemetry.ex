defmodule Quanta.Telemetry do
  @moduledoc """
  Telemetry event declarations for the Quanta runtime.

  All events use the `:telemetry` library (v1.3+). This module documents
  every event name the system emits (or will emit in future tickets) and
  provides thin helpers around `:telemetry.span/3` and `:telemetry.execute/3`.

  ## Emitted events

  ### Actor lifecycle

  * `[:quanta, :actor, :activate, :start | :stop | :exception]` — actor init
  * `[:quanta, :actor, :message, :start | :stop | :exception]` — message dispatch
  * `[:quanta, :actor, :passivate]` — idle or forced passivation
  * `[:quanta, :actor, :crash]` — init failure (with reason)

  ### Actor mailbox (already emitted by Server.check_mailbox)

  * `[:quanta, :actor, :mailbox, :warning | :shedding | :critical]`

  ### Distributed registry

  * `[:quanta, :actor, :conflict_resolved]` — registry conflict resolved between two nodes

  ### Rate limiting

  * `[:quanta, :rate_limit, :rejected]` — request rejected by rate limiter

  ### Custom (emitted by actor code via :emit_telemetry effect)

  * `[:quanta, :actor, :custom, ...]`

  ### Cluster membership

  * `[:quanta, :cluster, :node_up]` — a node joined the cluster
  * `[:quanta, :cluster, :node_down]` — a node left the cluster

  ### Broadway event processing

  * `[:quanta, :broadway, :success]` — batch of messages successfully processed
  * `[:quanta, :broadway, :failed]` — batch of messages that failed processing

  ## Future events (declared, not yet instrumented)

  * `[:quanta, :wasm, :call, :start | :stop | :exception]` — T06
  * `[:quanta, :nats, :publish, :start | :stop | :exception]` — T09
  * `[:quanta, :nats, :kv, :start | :stop | :exception]` — T07
  * `[:quanta, :state, :snapshot]` — T07 (event sourcing)
  * `[:quanta, :state, :replay]` — T07 (event sourcing)
  """

  @doc """
  No-op for now. Will attach default handlers when OTel SDK is added.
  """
  @spec setup() :: :ok
  def setup, do: :ok

  @doc """
  Wraps a function in `:telemetry.span/3`, emitting start/stop/exception events.

  Returns the result of `fun`.

  ## Example

      Quanta.Telemetry.span([:quanta, :actor, :message], %{actor_id: id}, fn ->
        result = do_work()
        {result, %{actor_id: id}}
      end)
  """
  @spec span(
          :telemetry.event_prefix(),
          :telemetry.event_metadata(),
          :telemetry.span_function()
        ) :: term()
  def span(event_prefix, metadata, fun) do
    :telemetry.span(event_prefix, metadata, fun)
  end

  @doc """
  Emits a discrete telemetry event via `:telemetry.execute/3`.
  """
  @spec emit(:telemetry.event_name(), :telemetry.event_measurements(), :telemetry.event_metadata()) ::
          :ok
  def emit(event, measurements, metadata) do
    :telemetry.execute(event, measurements, metadata)
  end
end
