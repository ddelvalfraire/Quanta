defmodule Quanta.Bench.Tier1.Skynet do
  @moduledoc """
  B1.4 — Skynet 1M actor tree benchmark.

  Creates a tree of 1M lightweight processes, each leaf sends its index
  up the tree. The root collects the sum. Measures actor creation and
  message aggregation throughput.
  """

  alias Quanta.Bench.Base

  @spec run :: :ok
  def run do
    Base.run("tier1_skynet", scenarios(), warmup: 1, time: 3)
  end

  defp scenarios do
    %{
      "skynet_100k" => fn -> skynet(0, 100_000, 10) end,
      "skynet_1m" => fn -> skynet(0, 1_000_000, 10) end
    }
  end

  defp skynet(num, 1, _div) do
    num
  end

  defp skynet(num, size, div) do
    child_size = div(size, div)
    parent = self()
    ref = make_ref()

    for i <- 0..(div - 1) do
      child_num = num + i * child_size

      spawn(fn ->
        result = skynet(child_num, child_size, div)
        send(parent, {:result, ref, result})
      end)
    end

    sum =
      Enum.reduce(1..div, 0, fn _, acc ->
        receive do
          {:result, ^ref, val} -> acc + val
        after
          60_000 -> raise "Skynet child timed out"
        end
      end)

    sum
  end
end
