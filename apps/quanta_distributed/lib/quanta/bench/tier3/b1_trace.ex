defmodule Quanta.Bench.Tier3.B1Trace do
  @moduledoc "B3.1 -- 260K sequential character inserts. SLO: < 5s."

  alias Quanta.Bench.Base
  alias Quanta.Nifs.LoroEngine

  @spec run :: :ok
  def run do
    Base.run("tier3_b1_trace", %{
      "260k_sequential_inserts" => fn ->
        {:ok, doc} = LoroEngine.doc_new()

        for i <- 0..259_999 do
          :ok = LoroEngine.text_insert(doc, "text", i, "x")
        end

        {:ok, 260_000} = LoroEngine.text_length(doc, "text")
      end
    }, warmup: 1, time: 10)
  end
end
