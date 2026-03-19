defmodule Quanta.Bench.Tier3.SnapshotRoundtrip do
  @moduledoc """
  B3.2 -- Export/import snapshot roundtrip benchmark.

  Creates a document with substantial state, then measures the time to export
  a snapshot and re-import it into a fresh document.

  SLO: p99 < 5 ms for snapshot export + import roundtrip.
  """

  alias Quanta.Bench.Base

  # Number of inserts to pre-populate the document with before benchmarking
  # alias Quanta.Nifs.LoroEngine
  # @doc_size 10_000

  @doc "Run the B3.2 snapshot roundtrip benchmark."
  @spec run :: :ok
  def run do
    Base.run("tier3_snapshot_roundtrip", scenarios(), warmup: 2, time: 5)
  end

  defp scenarios do
    # TODO: Pre-populate a doc with 10_000 inserts before benchmarking
    # {:ok, doc} = LoroEngine.doc_new()
    # for i <- 0..9_999, do: LoroEngine.text_insert(doc, "text", i, "a")

    %{
      "snapshot_export" => fn ->
        # TODO: Export snapshot from pre-populated doc
        # {:ok, _snapshot} = LoroEngine.doc_export_snapshot(doc)
        :ok
      end,
      "snapshot_import" => fn ->
        # TODO: Import a pre-exported snapshot into a fresh doc
        # {:ok, fresh} = LoroEngine.doc_new()
        # :ok = LoroEngine.doc_import(fresh, snapshot)
        :ok
      end,
      "snapshot_roundtrip" => fn ->
        # TODO: Export + import in one operation, measure combined time
        # {:ok, snap} = LoroEngine.doc_export_snapshot(doc)
        # {:ok, fresh} = LoroEngine.doc_new()
        # :ok = LoroEngine.doc_import(fresh, snap)
        :ok
      end
    }
  end
end
