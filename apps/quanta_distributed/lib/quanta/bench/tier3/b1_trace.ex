defmodule Quanta.Bench.Tier3.B1Trace do
  @moduledoc """
  B3.1 -- Apply 260K sequential inserts from a synthetic trace.

  Simulates a large editing session by inserting 260K single-character operations
  into a Loro text container and measures total wall-clock time.

  SLO: < 5 seconds for 260K inserts.
  """

  alias Quanta.Bench.Base
  alias Quanta.Nifs.LoroEngine

  @insert_count 260_000

  @doc "Run the B3.1 trace insertion benchmark."
  @spec run :: :ok
  def run do
    Base.run("tier3_b1_trace", scenarios(), warmup: 1, time: 10)
  end

  defp scenarios do
    %{
      "260k_sequential_inserts" => fn ->
        {:ok, doc} = LoroEngine.doc_new()

        for i <- 0..(@insert_count - 1) do
          # TODO: Insert a character at position i to simulate sequential typing
          # LoroEngine.text_insert(doc, "text", i, "x")
          _ = i
        end

        # TODO: Assert final text length == @insert_count
        # {:ok, len} = LoroEngine.text_length(doc, "text")
        doc
      end
    }
  end
end
