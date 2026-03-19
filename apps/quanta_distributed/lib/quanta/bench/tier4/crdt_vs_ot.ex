defmodule Quanta.Bench.Tier4.CrdtVsOt do
  @moduledoc "B4.4 -- CRDT merge vs simulated OT transform. SLO: <2x overhead."

  alias Quanta.Bench.Base
  alias Quanta.Nifs.LoroEngine

  @edit_count 5_000

  @spec run :: :ok
  def run do
    Base.run("tier4_crdt_vs_ot", %{
      "crdt_sequential_merge" => fn ->
        {:ok, a} = LoroEngine.doc_new_with_peer_id(1)
        {:ok, b} = LoroEngine.doc_new_with_peer_id(2)

        for i <- 0..div(@edit_count, 2) - 1 do
          :ok = LoroEngine.text_insert(a, "text", 0, "a")
        end

        for i <- 0..div(@edit_count, 2) - 1 do
          :ok = LoroEngine.text_insert(b, "text", 0, "b")
        end

        {:ok, snap_a} = LoroEngine.doc_export_snapshot(a)
        {:ok, snap_b} = LoroEngine.doc_export_snapshot(b)

        :ok = LoroEngine.doc_apply_delta(a, snap_b)
        :ok = LoroEngine.doc_apply_delta(b, snap_a)

        {:ok, val_a} = LoroEngine.doc_get_value(a)
        {:ok, val_b} = LoroEngine.doc_get_value(b)
        true = val_a == val_b
      end,
      "ot_simulated_transform" => fn ->
        # Simulate OT: sequential transform of @edit_count operations
        # Each transform is an O(1) list operation (best-case OT)
        doc = :array.new(@edit_count, default: 0)

        Enum.reduce(0..(@edit_count - 1), doc, fn i, acc ->
          :array.set(i, ?a + rem(i, 26), acc)
        end)
      end,
      "crdt_doc_size_overhead" => fn ->
        {:ok, doc} = LoroEngine.doc_new()

        for i <- 0..(@edit_count - 1) do
          :ok = LoroEngine.text_insert(doc, "text", i, "x")
        end

        {:ok, snap} = LoroEngine.doc_export_snapshot(doc)
        {:ok, text} = LoroEngine.text_to_string(doc, "text")
        _overhead_ratio = byte_size(snap) / byte_size(text)
      end
    }, warmup: 1, time: 10)
  end
end
