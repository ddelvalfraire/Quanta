defmodule Quanta.Actor.EphemeralStore do
  @moduledoc """
  TTL-based in-memory store backed by ETS.

  Used by CRDT actors for ephemeral data such as cursor positions
  and presence information (T28).
  """

  @type tid :: :ets.tid()

  @spec new(pos_integer()) :: {:ok, tid()}
  def new(ttl_ms) when is_integer(ttl_ms) and ttl_ms > 0 do
    tid = :ets.new(:ephemeral_store, [:set, :public])
    :ets.insert(tid, {:__ttl_ms__, ttl_ms})
    {:ok, tid}
  end

  @spec put(tid(), term(), term()) :: :ok
  def put(tid, key, value) do
    now = System.monotonic_time(:millisecond)
    :ets.insert(tid, {key, value, now})
    :ok
  end

  @spec get(tid(), term()) :: {:ok, term()} | :expired | :not_found
  def get(tid, key) do
    case :ets.lookup(tid, key) do
      [{^key, value, written_at}] ->
        [{:__ttl_ms__, ttl_ms}] = :ets.lookup(tid, :__ttl_ms__)
        now = System.monotonic_time(:millisecond)

        if now - written_at <= ttl_ms do
          {:ok, value}
        else
          :ets.delete(tid, key)
          :expired
        end

      [] ->
        :not_found
    end
  end

  @spec delete(tid(), term()) :: :ok
  def delete(tid, key) do
    :ets.delete(tid, key)
    :ok
  end

  @spec cleanup(tid()) :: :ok
  def cleanup(tid) do
    [{:__ttl_ms__, ttl_ms}] = :ets.lookup(tid, :__ttl_ms__)
    cutoff = System.monotonic_time(:millisecond) - ttl_ms

    :ets.select_delete(tid, [
      {{:"$1", :_, :"$2"}, [{:andalso, {:"=/=", :"$1", :__ttl_ms__}, {:<, :"$2", cutoff}}], [true]}
    ])

    :ok
  end

  @spec destroy(tid()) :: :ok
  def destroy(tid) do
    :ets.delete(tid)
    :ok
  end
end
