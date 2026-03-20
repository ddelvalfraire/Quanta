defmodule Quanta.Bench.Tier3.DeltaMerge do
  @moduledoc "B3.5 -- Cross-replica delta merge. SLO: p99 < 1ms for 1K ops."

  alias Quanta.Bench.Base
  alias Quanta.Nifs.LoroEngine

  @spec run :: :ok
  def run do
    # Pre-build a delta of 1K ops
    {:ok, src} = LoroEngine.doc_new_with_peer_id(1)
    {:ok, v0} = LoroEngine.doc_version(src)

    for i <- 0..999 do
      :ok = LoroEngine.text_insert(src, "text", i, "d")
    end

    {:ok, delta} = LoroEngine.doc_export_updates_from(src, v0)

    Base.run("tier3_delta_merge", %{
      "delta_export_1k_ops" => fn ->
        {:ok, doc} = LoroEngine.doc_new_with_peer_id(10)
        {:ok, v} = LoroEngine.doc_version(doc)

        for i <- 0..999, do: :ok = LoroEngine.text_insert(doc, "text", i, "e")

        {:ok, _} = LoroEngine.doc_export_updates_from(doc, v)
      end,
      "delta_import_1k_ops" => fn ->
        {:ok, fresh} = LoroEngine.doc_new()
        :ok = LoroEngine.doc_apply_delta(fresh, delta)
      end,
      "bidirectional_sync" => fn ->
        {:ok, a} = LoroEngine.doc_new_with_peer_id(100)
        {:ok, b} = LoroEngine.doc_new_with_peer_id(200)

        for _i <- 0..499, do: :ok = LoroEngine.text_insert(a, "text", 0, "a")
        for _i <- 0..499, do: :ok = LoroEngine.text_insert(b, "text", 0, "b")

        {:ok, snap_a} = LoroEngine.doc_export_snapshot(a)
        {:ok, snap_b} = LoroEngine.doc_export_snapshot(b)

        :ok = LoroEngine.doc_apply_delta(a, snap_b)
        :ok = LoroEngine.doc_apply_delta(b, snap_a)
      end
    }, warmup: 2, time: 5)
  end
end
