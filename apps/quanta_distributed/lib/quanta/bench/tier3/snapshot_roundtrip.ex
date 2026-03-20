defmodule Quanta.Bench.Tier3.SnapshotRoundtrip do
  @moduledoc "B3.2 -- Snapshot export/import roundtrip. SLO: p99 < 5ms."

  alias Quanta.Bench.Base
  alias Quanta.Nifs.LoroEngine

  @spec run :: :ok
  def run do
    # Pre-populate a doc with 10K inserts
    {:ok, doc} = LoroEngine.doc_new()
    for i <- 0..9_999, do: :ok = LoroEngine.text_insert(doc, "text", i, "a")
    {:ok, snapshot} = LoroEngine.doc_export_snapshot(doc)

    Base.run("tier3_snapshot_roundtrip", %{
      "snapshot_export" => fn ->
        {:ok, _} = LoroEngine.doc_export_snapshot(doc)
      end,
      "snapshot_import" => fn ->
        {:ok, fresh} = LoroEngine.doc_new()
        :ok = LoroEngine.doc_import(fresh, snapshot)
      end,
      "snapshot_roundtrip" => fn ->
        {:ok, snap} = LoroEngine.doc_export_snapshot(doc)
        {:ok, fresh} = LoroEngine.doc_new()
        :ok = LoroEngine.doc_import(fresh, snap)
      end
    }, warmup: 2, time: 5)
  end
end
