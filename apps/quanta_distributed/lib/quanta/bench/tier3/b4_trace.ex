defmodule Quanta.Bench.Tier3.B4Trace do
  @moduledoc "B3.4 -- Synthetic editing trace replay (100K mixed ops). SLO: < 3s."

  alias Quanta.Bench.Base
  alias Quanta.Nifs.LoroEngine

  @spec run :: :ok
  def run do
    Base.run("tier3_b4_trace", %{
      "100k_mixed_ops" => fn ->
        {:ok, doc} = LoroEngine.doc_new()

        # 80K inserts + 20K map sets to simulate mixed editing
        for _i <- 0..79_999 do
          :ok = LoroEngine.text_insert(doc, "text", 0, "c")
        end

        for i <- 0..19_999 do
          :ok = LoroEngine.map_set(doc, "meta", "key_#{rem(i, 100)}", i)
        end

        {:ok, _} = LoroEngine.doc_get_value(doc)
      end
    }, warmup: 1, time: 10)
  end
end
