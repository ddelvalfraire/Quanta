# Standalone Quanta benchmark — runs without the full app stack.
# Outputs JSON to stdout in the same format as competitors.

defmodule Bench do
  @iterations %{
    ping_pong_1k: 200,
    ping_pong_10k: 100,
    fan_out_10: 200,
    fan_out_100: 100,
    fan_out_1000: 50,
    skynet: 10,
    activation: 1000
  }

  def run do
    results = %{
      framework: "quanta",
      benchmarks:
        Map.merge(run_ping_pong(), run_fan_out())
        |> Map.merge(run_skynet())
        |> Map.merge(run_activation())
    }

    IO.puts(Jason.encode!(results))
  end

  defp run_ping_pong do
    %{
      ping_pong_1k: measure(@iterations.ping_pong_1k, fn -> do_ping_pong(1_000) end),
      ping_pong_10k: measure(@iterations.ping_pong_10k, fn -> do_ping_pong(10_000) end)
    }
  end

  defp run_fan_out do
    %{
      fan_out_10: measure(@iterations.fan_out_10, fn -> do_fan_out(10) end),
      fan_out_100: measure(@iterations.fan_out_100, fn -> do_fan_out(100) end),
      fan_out_1000: measure(@iterations.fan_out_1000, fn -> do_fan_out(1_000) end)
    }
  end

  defp run_skynet do
    %{
      skynet_1m: measure(@iterations.skynet, 5, fn -> do_skynet(0, 1_000_000, 10) end)
    }
  end

  defp run_activation do
    %{
      cold_activation: measure(@iterations.activation, fn ->
        parent = self()
        ref = make_ref()

        spawn_link(fn ->
          receive do
            {:cmd, :inc, from, r} -> send(from, {:ok, r, 1})
          end
        end)
        |> then(fn pid -> send(pid, {:cmd, :inc, parent, ref}) end)

        receive do
          {:ok, ^ref, 1} -> :ok
        end
      end),
      warm_message: measure_warm()
    }
  end

  defp measure_warm do
    actor = spawn_link(fn -> warm_loop() end)
    for _ <- 1..100 do
      ref = make_ref()
      send(actor, {:cmd, :inc, self(), ref})
      receive do: ({:ok, ^ref, _} -> :ok)
    end

    result = measure(@iterations.activation, fn ->
      ref = make_ref()
      send(actor, {:cmd, :inc, self(), ref})
      receive do: ({:ok, ^ref, _} -> :ok)
    end)

    send(actor, :stop)
    result
  end

  defp warm_loop do
    receive do
      {:cmd, :inc, from, ref} ->
        send(from, {:ok, ref, 1})
        warm_loop()
      :stop -> :ok
    end
  end

  defp do_ping_pong(n) do
    parent = self()
    ref = make_ref()
    pong = spawn(fn -> pong_loop(0, n, parent, ref) end)
    ping_loop(pong, 0, n)
    receive do: ({:done, ^ref} -> :ok)
  end

  defp ping_loop(_pong, n, n), do: :ok
  defp ping_loop(pong, i, n) do
    send(pong, {:ping, self()})
    receive do: (:pong -> ping_loop(pong, i + 1, n))
  end

  defp pong_loop(n, n, parent, ref), do: send(parent, {:done, ref})
  defp pong_loop(i, n, parent, ref) do
    receive do: ({:ping, from} -> send(from, :pong); pong_loop(i + 1, n, parent, ref))
  end

  defp do_fan_out(n) do
    parent = self()
    ref = make_ref()
    for _ <- 1..n do
      spawn(fn -> receive do: ({:msg, ^ref} -> send(parent, {:ack, ref})) end)
    end
    |> Enum.each(&send(&1, {:msg, ref}))
    for _ <- 1..n, do: receive(do: ({:ack, ^ref} -> :ok))
  end

  defp do_skynet(num, 1, _div), do: num
  defp do_skynet(num, size, div) do
    parent = self()
    ref = make_ref()
    child_size = Kernel.div(size, div)
    for i <- 0..(div - 1) do
      spawn(fn ->
        result = do_skynet(num + i * child_size, child_size, div)
        send(parent, {:result, ref, result})
      end)
    end
    Enum.reduce(1..div, 0, fn _, acc ->
      receive do: ({:result, ^ref, val} -> acc + val)
    end)
  end

  defp measure(n, fun), do: measure(n, 20, fun)

  defp measure(n, warmup, fun) do
    for _ <- 1..warmup, do: fun.()

    times =
      for _ <- 1..n do
        t0 = System.monotonic_time(:microsecond)
        fun.()
        System.monotonic_time(:microsecond) - t0
      end
      |> Enum.sort()

    %{
      iterations: n,
      mean_us: Float.round(Enum.sum(times) / n, 2),
      median_us: Enum.at(times, div(n, 2)),
      p99_us: Enum.at(times, trunc(n * 0.99)),
      min_us: List.first(times),
      max_us: List.last(times),
      ips: Float.round(n / (Enum.sum(times) / 1_000_000), 2)
    }
  end
end

Bench.run()
