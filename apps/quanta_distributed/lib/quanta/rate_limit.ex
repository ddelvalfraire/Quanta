defmodule Quanta.RateLimit do
  @moduledoc """
  ETS-based token bucket rate limiter. No GenServer — pure atomic counters.

  Two tiers checked in order:
  1. Per-instance: `{namespace, type, id}` — `messages_per_second`
  2. Per-type aggregate: `{namespace, type}` — `messages_per_second_type`

  Each bucket stores `{key, tokens_remaining, window_start_ms}`.
  Tokens refill at the start of each 1-second window.
  """

  @table __MODULE__

  @spec init() :: :ok
  def init do
    :ets.new(@table, [
      :named_table,
      :public,
      :set,
      read_concurrency: true,
      write_concurrency: true
    ])

    :ok
  end

  @spec check(Quanta.ActorId.t(), Quanta.Manifest.t()) ::
          :ok | {:error, :rate_limited, pos_integer()}
  def check(%Quanta.ActorId{} = actor_id, %Quanta.Manifest{} = manifest) do
    now_ms = System.monotonic_time(:millisecond)

    instance_key = {:instance, actor_id.namespace, actor_id.type, actor_id.id}
    instance_limit = manifest.rate_limits.messages_per_second

    case try_consume(instance_key, instance_limit, now_ms) do
      :ok ->
        type_key = {:type, actor_id.namespace, actor_id.type}
        type_limit = manifest.rate_limits.messages_per_second_type

        case try_consume(type_key, type_limit, now_ms) do
          :ok -> :ok
          :denied -> {:error, :rate_limited, retry_after(now_ms)}
        end

      :denied ->
        {:error, :rate_limited, retry_after(now_ms)}
    end
  end

  @spec reset(Quanta.ActorId.t()) :: :ok
  def reset(%Quanta.ActorId{} = actor_id) do
    instance_key = {:instance, actor_id.namespace, actor_id.type, actor_id.id}
    type_key = {:type, actor_id.namespace, actor_id.type}
    :ets.delete(@table, instance_key)
    :ets.delete(@table, type_key)
    :ok
  end

  # ETS tuple: {key, tokens_remaining, window_start_ms}

  defp try_consume(key, limit, now_ms) do
    case :ets.lookup(@table, key) do
      [{_key, _tokens, window_start}] when now_ms - window_start < 1000 ->
        # {position, decrement, threshold, set_value} — atomic, floor at -1
        case :ets.update_counter(@table, key, {2, -1, -1, -1}) do
          n when n >= 0 -> :ok
          _ -> :denied
        end

      _ ->
        :ets.insert(@table, {key, limit - 1, now_ms})
        :ok
    end
  end

  defp retry_after(now_ms) do
    window_remainder = 1000 - rem(abs(now_ms), 1000)
    max(window_remainder, 1)
  end
end
