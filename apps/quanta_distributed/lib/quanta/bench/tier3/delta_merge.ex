defmodule Quanta.Bench.Tier3.DeltaMerge do
  @moduledoc """
  B3.5 -- Cross-replica delta merge benchmark.

  Measures the cost of exporting deltas from one replica and applying them to
  another, simulating the sync protocol between distributed nodes.

  SLO: delta export + import for 1K ops < 1 ms p99.
  """

  alias Quanta.Bench.Base

  # alias Quanta.Nifs.LoroEngine
  # @ops_per_batch 1_000

  @doc "Run the B3.5 delta merge benchmark."
  @spec run :: :ok
  def run do
    Base.run("tier3_delta_merge", scenarios(), warmup: 2, time: 5)
  end

  defp scenarios do
    %{
      "delta_export_1k_ops" => fn ->
        # TODO: Create doc, insert 1_000 chars, export delta
        # {:ok, doc} = LoroEngine.doc_new()
        # {:ok, v0} = LoroEngine.doc_version(doc)
        # for i <- 0..999, do: LoroEngine.text_insert(doc, "text", i, "a")
        # {:ok, _delta} = LoroEngine.doc_export_updates_from(doc, v0)
        :ok
      end,
      "delta_import_1k_ops" => fn ->
        # TODO: Apply a pre-exported delta to a fresh doc
        # {:ok, fresh} = LoroEngine.doc_new()
        # :ok = LoroEngine.doc_apply_delta(fresh, delta)
        :ok
      end,
      "bidirectional_sync" => fn ->
        # TODO: Two replicas each make edits, then sync bidirectionally
        # Replica A: 500 inserts, Replica B: 500 inserts
        # Export delta from A, import into B; export delta from B, import into A
        # Verify both replicas have identical state
        :ok
      end
    }
  end
end
