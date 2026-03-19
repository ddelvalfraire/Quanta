defmodule Quanta.Bench.Tier4.MultiRegionSim do
  @moduledoc "B4.5 -- Simulated multi-region CRDT sync with injected latency."

  alias Quanta.Bench.Base
  alias Quanta.Nifs.LoroEngine

  @spec run :: :ok
  def run do
    Base.run("tier4_multi_region_sim", %{
      "region_rtt_50ms" => fn -> sync_with_latency(25) end,
      "region_rtt_100ms" => fn -> sync_with_latency(50) end,
      "region_rtt_200ms" => fn -> sync_with_latency(100) end
    }, warmup: 1, time: 10)
  end

  defp sync_with_latency(one_way_ms) do
    parent = self()
    ref = make_ref()

    # Region A: make edits, export, "send" with latency
    {:ok, a} = LoroEngine.doc_new_with_peer_id(1)

    for i <- 0..99 do
      :ok = LoroEngine.text_insert(a, "text", 0, "a")
    end

    {:ok, snap_a} = LoroEngine.doc_export_snapshot(a)

    # Simulate one-way network latency
    Process.sleep(one_way_ms)

    # Region B: receive, apply, make own edits, send back
    {:ok, b} = LoroEngine.doc_new_with_peer_id(2)

    for i <- 0..99 do
      :ok = LoroEngine.text_insert(b, "text", 0, "b")
    end

    :ok = LoroEngine.doc_apply_delta(b, snap_a)
    {:ok, snap_b} = LoroEngine.doc_export_snapshot(b)

    # Return trip
    Process.sleep(one_way_ms)

    # Region A: apply B's changes
    :ok = LoroEngine.doc_apply_delta(a, snap_b)

    # Verify convergence
    {:ok, val_a} = LoroEngine.doc_get_value(a)
    {:ok, val_b} = LoroEngine.doc_get_value(b)
    true = val_a == val_b
  end
end
