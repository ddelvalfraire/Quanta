defmodule Quanta.Bench.Tier3.ConcurrentEditing do
  @moduledoc "B3.3 -- N concurrent editors, merge into one doc."

  alias Quanta.Bench.Base
  alias Quanta.Nifs.LoroEngine

  @edits_per_editor 100

  @spec run :: :ok
  def run do
    Base.run("tier3_concurrent_editing", %{
      "2_editors" => fn -> run_editors(2) end,
      "10_editors" => fn -> run_editors(10) end,
      "50_editors" => fn -> run_editors(50) end
    }, warmup: 1, time: 10)
  end

  defp run_editors(n) do
    # Each editor gets its own doc with a unique peer_id
    docs =
      for i <- 0..(n - 1) do
        {:ok, doc} = LoroEngine.doc_new_with_peer_id(i)

        for j <- 0..(@edits_per_editor - 1) do
          :ok = LoroEngine.text_insert(doc, "text", 0, "#{i}#{j}")
        end

        doc
      end

    # Export snapshots from all editors
    snapshots = Enum.map(docs, fn doc ->
      {:ok, snap} = LoroEngine.doc_export_snapshot(doc)
      snap
    end)

    # Merge all into a single target
    {:ok, target} = LoroEngine.doc_new()

    for snap <- snapshots do
      :ok = LoroEngine.doc_import(target, snap)
    end

    {:ok, _val} = LoroEngine.doc_get_value(target)
  end
end
