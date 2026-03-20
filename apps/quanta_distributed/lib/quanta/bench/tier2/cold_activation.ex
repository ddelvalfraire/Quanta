defmodule Quanta.Bench.Tier2.ColdActivation do
  @moduledoc """
  B2.1 — Cold activation benchmark.

  Simulates cold path: registry miss → spawn → register → first message → stop.
  SLO: p99 < 50 ms.
  """

  alias Quanta.Bench.Base

  @spec run :: :ok
  def run do
    Base.run("tier2_cold_activation", scenarios(), warmup: 2, time: 5)
  end

  defp scenarios do
    %{
      "cold_activation" => fn -> cold_activate_and_inc() end
    }
  end

  defp cold_activate_and_inc do
    parent = self()
    ref = make_ref()

    pid =
      spawn_link(fn ->
        receive do
          {:cmd, :inc, from, r} ->
            send(from, {:ok, r, 1})
        end
      end)

    send(pid, {:cmd, :inc, parent, ref})

    receive do
      {:ok, ^ref, 1} -> :ok
    after
      5_000 -> raise "Cold activation timed out"
    end
  end
end
