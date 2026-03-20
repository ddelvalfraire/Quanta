defmodule Quanta.Bench.Tier1.ConcurrentMailbox do
  @moduledoc """
  B1.5 — Concurrent mailbox contention benchmark.

  N producers send messages to a single consumer. Measures throughput
  under contention (N→1 pattern).
  """

  alias Quanta.Bench.Base

  @spec run :: :ok
  def run do
    Base.run("tier1_concurrent_mailbox", scenarios(), warmup: 2, time: 5)
  end

  defp scenarios do
    %{
      "N=10_msgs=1000" => fn -> run_contention(10, 1_000) end,
      "N=100_msgs=100" => fn -> run_contention(100, 100) end,
      "N=1000_msgs=10" => fn -> run_contention(1_000, 10) end
    }
  end

  defp run_contention(producers, msgs_per_producer) do
    total = producers * msgs_per_producer
    parent = self()
    ref = make_ref()

    # Consumer: counts received messages
    consumer =
      spawn(fn ->
        count = consumer_loop(0, total)
        send(parent, {:done, ref, count})
      end)

    # Producers: each sends msgs_per_producer messages
    for _ <- 1..producers do
      spawn_link(fn ->
        for i <- 1..msgs_per_producer do
          send(consumer, {:msg, i})
        end
      end)
    end

    receive do
      {:done, ^ref, ^total} -> :ok
    after
      30_000 -> raise "Contention benchmark timed out"
    end
  end

  defp consumer_loop(count, target) when count >= target, do: count

  defp consumer_loop(count, target) do
    receive do
      {:msg, _} -> consumer_loop(count + 1, target)
    end
  end
end
