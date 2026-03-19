defmodule Quanta.Bench.Tier3.ConcurrentEditing do
  @moduledoc """
  B3.3 -- N concurrent editors on the same document.

  Measures throughput and merge cost when N independent editors produce deltas
  that are merged into a single document. Each editor operates on its own
  replica and periodically syncs via delta export/import.

  SLO: linear scaling up to 10 editors, < 2x overhead at 50 editors.
  """

  alias Quanta.Bench.Base

  # alias Quanta.Nifs.LoroEngine
  @edits_per_editor 1_000

  @doc "Run the B3.3 concurrent editing benchmark."
  @spec run :: :ok
  def run do
    Base.run("tier3_concurrent_editing", scenarios(), warmup: 1, time: 10)
  end

  defp scenarios do
    %{
      "2_editors" => fn -> run_concurrent_editors(2) end,
      "10_editors" => fn -> run_concurrent_editors(10) end,
      "50_editors" => fn -> run_concurrent_editors(50) end
    }
  end

  defp run_concurrent_editors(n) do
    # TODO: Create n docs, each with a unique peer_id
    # docs = for i <- 0..(n - 1) do
    #   {:ok, doc} = LoroEngine.doc_new_with_peer_id(i)
    #   doc
    # end

    # TODO: Each editor inserts @edits_per_editor chars into its own replica
    # TODO: Export deltas from each editor
    # TODO: Import all deltas into a single merge target
    # TODO: Verify merge target has all edits
    _ = {n, @edits_per_editor}
    :ok
  end
end
