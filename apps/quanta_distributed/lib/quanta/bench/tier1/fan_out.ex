defmodule Quanta.Bench.Tier1.FanOut do
  @moduledoc """
  B1.2 — Fan-out broadcast benchmark.

  Measures 1→N broadcast latency for N=10, 100, 1000.
  """

  alias Quanta.Bench.Base

  @doc "Run the fan-out benchmark."
  @spec run :: :ok
  def run do
    Base.run("tier1_fan_out", scenarios(), warmup: 2, time: 5)
  end

  defp scenarios do
    %{
      "fan_out_10" => fn -> run_fan_out(10) end,
      "fan_out_100" => fn -> run_fan_out(100) end,
      "fan_out_1000" => fn -> run_fan_out(1_000) end
    }
  end

  defp run_fan_out(n) do
    parent = self()
    ref = make_ref()

    receivers =
      for _ <- 1..n do
        spawn(fn ->
          receive do
            {:msg, ^ref} -> send(parent, {:ack, ref})
          end
        end)
      end

    # Broadcast
    for pid <- receivers, do: send(pid, {:msg, ref})

    # Wait for all acks
    for _ <- 1..n do
      receive do
        {:ack, ^ref} -> :ok
      after
        30_000 -> raise "Fan-out timed out"
      end
    end
  end
end
