defmodule Mix.Tasks.Bench.Competitor do
  @moduledoc "Run Quanta benchmarks in the same format as competitor harnesses."
  @shortdoc "Run comparable benchmarks, output JSON"

  use Mix.Task

  alias Quanta.Actor.{CommandRouter, ManifestRegistry}
  alias Quanta.{ActorId, Envelope, Manifest}

  @namespace "bench"
  @type_name "counter"

  @iterations %{
    ping_pong_1k: 200,
    ping_pong_10k: 100,
    fan_out_10: 200,
    fan_out_100: 100,
    fan_out_1000: 50,
    skynet: 10,
    cold_activation: 1000,
    warm_message: 1000
  }

  @impl true
  def run(_args) do
    Mix.Task.run("app.start")

    register_bench_actor!()

    results = %{
      framework: "quanta",
      benchmarks:
        Map.merge(
          run_ping_pong(),
          run_fan_out()
        )
        |> Map.merge(run_skynet())
        |> Map.merge(run_activation())
    }

    path = Path.join([File.cwd!(), "benchmarks", "results", "quanta.json"])
    File.mkdir_p!(Path.dirname(path))
    File.write!(path, Jason.encode!(results, pretty: true))
    IO.puts("Results written to #{path}")
  end

  # ---------------------------------------------------------------------------
  # Setup: register a bench manifest + module so CommandRouter can route to it
  # ---------------------------------------------------------------------------

  defp register_bench_actor! do
    manifest = %Manifest{version: "1", namespace: @namespace, type: @type_name}
    :ok = ManifestRegistry.put(manifest)

    prev = Application.get_env(:quanta_distributed, :actor_modules, %{})

    Application.put_env(
      :quanta_distributed,
      :actor_modules,
      Map.put(prev, {@namespace, @type_name}, Quanta.Test.Actors.Counter)
    )
  end

  # ---------------------------------------------------------------------------
  # Quanta actor framework benchmarks (cold_activation, warm_message)
  # ---------------------------------------------------------------------------

  defp run_activation do
    %{
      cold_activation: measure(@iterations.cold_activation, fn i ->
        actor_id = %ActorId{namespace: @namespace, type: @type_name, id: "cold-#{i}"}
        envelope = Envelope.new(payload: "inc", sender: :system)
        {:ok, _} = CommandRouter.route(actor_id, envelope)
      end),
      warm_message: measure_warm()
    }
  end

  defp measure_warm do
    # Activate a single actor first
    actor_id = %ActorId{namespace: @namespace, type: @type_name, id: "warm-persistent"}
    envelope = Envelope.new(payload: "inc", sender: :system)
    {:ok, _} = CommandRouter.route(actor_id, envelope)

    # Warmup
    for _ <- 1..100 do
      {:ok, _} = CommandRouter.route(actor_id, Envelope.new(payload: "inc", sender: :system))
    end

    measure(@iterations.warm_message, fn _i ->
      {:ok, _} = CommandRouter.route(actor_id, Envelope.new(payload: "inc", sender: :system))
    end)
  end

  # ---------------------------------------------------------------------------
  # Raw BEAM benchmarks (ping_pong, fan_out, skynet)
  #
  # These measure core BEAM runtime performance — raw process spawn/send/receive.
  # They are intentionally NOT routed through the Quanta actor framework.
  # ---------------------------------------------------------------------------

  defp run_ping_pong do
    %{
      ping_pong_1k: measure(@iterations.ping_pong_1k, fn _i -> do_ping_pong(1_000) end),
      ping_pong_10k: measure(@iterations.ping_pong_10k, fn _i -> do_ping_pong(10_000) end)
    }
  end

  defp run_fan_out do
    %{
      fan_out_10: measure(@iterations.fan_out_10, fn _i -> do_fan_out(10) end),
      fan_out_100: measure(@iterations.fan_out_100, fn _i -> do_fan_out(100) end),
      fan_out_1000: measure(@iterations.fan_out_1000, fn _i -> do_fan_out(1_000) end)
    }
  end

  defp run_skynet do
    %{
      skynet_1m: measure(@iterations.skynet, fn _i -> do_skynet(0, 1_000_000, 10) end)
    }
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
      spawn(fn ->
        receive do: ({:msg, ^ref} -> send(parent, {:ack, ref}))
      end)
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

  # ---------------------------------------------------------------------------
  # Measurement harness
  # ---------------------------------------------------------------------------

  defp measure(n, fun) do
    # Warmup
    for i <- 1..min(n, 10), do: fun.(i)

    times =
      for i <- 1..n do
        t0 = System.monotonic_time(:microsecond)
        fun.(i)
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
