defmodule Quanta.Bench.Tier1.PingPong do
  @moduledoc """
  B1.1 — Ping-pong benchmark.

  Measures raw message throughput between two actors (local and remote).
  SLO: >100K msg/sec local ping-pong.
  """

  alias Quanta.Bench.Base

  @doc "Run the ping-pong benchmark."
  @spec run :: :ok
  def run do
    Base.run("tier1_ping_pong", scenarios(), warmup: 2, time: 5)
  end

  defp scenarios do
    %{
      "local_ping_pong_1k" => fn ->
        run_ping_pong(1_000)
      end,
      "local_ping_pong_10k" => fn ->
        run_ping_pong(10_000)
      end
    }
  end

  defp run_ping_pong(n) do
    parent = self()
    ref = make_ref()

    pong =
      spawn(fn ->
        pong_loop(0, n, parent, ref)
      end)

    ping_loop(pong, 0, n)

    receive do
      {:done, ^ref} -> :ok
    after
      30_000 -> raise "Ping-pong timed out"
    end
  end

  defp ping_loop(_pong, n, n), do: :ok

  defp ping_loop(pong, i, n) do
    send(pong, {:ping, self()})

    receive do
      :pong -> ping_loop(pong, i + 1, n)
    end
  end

  defp pong_loop(n, n, parent, ref) do
    send(parent, {:done, ref})
  end

  defp pong_loop(i, n, parent, ref) do
    receive do
      {:ping, from} ->
        send(from, :pong)
        pong_loop(i + 1, n, parent, ref)
    end
  end
end
