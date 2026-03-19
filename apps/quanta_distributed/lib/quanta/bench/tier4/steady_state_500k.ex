defmodule Quanta.Bench.Tier4.SteadyState500k do
  @moduledoc """
  B4.3 -- 500K actor steady state benchmark.

  Measures memory footprint and message throughput with 500K actors registered
  in the system. Used to compare against Akka, Orleans, and Proto.Actor.

  SLO: < 2 KB memory per actor, > 100K msg/sec sustained.
  """

  alias Quanta.Bench.Base

  @actor_count 500_000

  @doc "Run the B4.3 steady state benchmark."
  @spec run :: :ok
  def run do
    Base.run("tier4_steady_state_500k", scenarios(), warmup: 1, time: 10)
  end

  defp scenarios do
    %{
      "register_500k" => fn ->
        # TODO: Register @actor_count lightweight actors (spawn + register in ETS)
        # TODO: Measure total memory, compute per-actor overhead
        # TODO: Assert per-actor memory < 2048 bytes
        _ = @actor_count
        :ok
      end,
      "message_throughput_500k" => fn ->
        # TODO: With 500K actors registered, send messages to random subset
        # TODO: Measure messages/sec sustained over 10s window
        :ok
      end
    }
  end
end
