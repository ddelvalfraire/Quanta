defmodule Quanta.Bench.Tier4.MultiRegionSim do
  @moduledoc "B4.5 -- Simulated multi-region CRDT sync with injected latency."

  alias Quanta.Bench.Base
  alias Quanta.Nifs.LoroEngine

  @spec run :: :ok
  def run do
    # Pre-compute snapshots with simulated latency OUTSIDE the timed section.
    # Each scenario only times the CRDT merge operations.
    {snap_a_50, snap_b_50} = build_snapshots(25)
    {snap_a_100, snap_b_100} = build_snapshots(50)
    {snap_a_200, snap_b_200} = build_snapshots(100)

    Base.run("tier4_multi_region_sim", %{
      "region_rtt_50ms" => fn -> merge_snapshots(snap_a_50, snap_b_50) end,
      "region_rtt_100ms" => fn -> merge_snapshots(snap_a_100, snap_b_100) end,
      "region_rtt_200ms" => fn -> merge_snapshots(snap_a_200, snap_b_200) end
    }, warmup: 1, time: 10)
  end

  # Builds docs, applies simulated latency, exports snapshots.
  # This work is NOT timed by Benchee.
  defp build_snapshots(one_way_ms) do
    # Region A: make edits, export
    {:ok, a} = LoroEngine.doc_new_with_peer_id(1)

    for _i <- 0..99 do
      :ok = LoroEngine.text_insert(a, "text", 0, "a")
    end

    {:ok, snap_a} = LoroEngine.doc_export_snapshot(a)

    # Simulate one-way network latency (outside timed section)
    Process.sleep(one_way_ms)

    # Region B: receive A's snapshot, make own edits, export
    {:ok, b} = LoroEngine.doc_new_with_peer_id(2)

    for _i <- 0..99 do
      :ok = LoroEngine.text_insert(b, "text", 0, "b")
    end

    :ok = LoroEngine.doc_apply_delta(b, snap_a)
    {:ok, snap_b} = LoroEngine.doc_export_snapshot(b)

    # Simulate return trip latency (outside timed section)
    Process.sleep(one_way_ms)

    {snap_a, snap_b}
  end

  # Only merge work is timed by Benchee
  defp merge_snapshots(snap_a, snap_b) do
    # Region A: apply both snapshots and merge
    {:ok, a} = LoroEngine.doc_new_with_peer_id(1)
    :ok = LoroEngine.doc_apply_delta(a, snap_a)
    :ok = LoroEngine.doc_apply_delta(a, snap_b)

    # Region B: apply both snapshots and merge
    {:ok, b} = LoroEngine.doc_new_with_peer_id(2)
    :ok = LoroEngine.doc_apply_delta(b, snap_b)
    :ok = LoroEngine.doc_apply_delta(b, snap_a)

    # Verify convergence
    {:ok, val_a} = LoroEngine.doc_get_value(a)
    {:ok, val_b} = LoroEngine.doc_get_value(b)
    true = val_a == val_b
  end
end
