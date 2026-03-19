defmodule Quanta.Bench.Tier4.WasmOverhead do
  @moduledoc "B4.2 -- NIF call overhead (proxy for WASM dispatch). SLO: <10us/invocation."

  alias Quanta.Bench.Base
  alias Quanta.Nifs.LoroEngine

  @spec run :: :ok
  def run do
    Base.run("tier4_wasm_overhead", %{
      "nif_doc_new" => fn ->
        {:ok, _} = LoroEngine.doc_new()
      end,
      "nif_text_insert" => fn ->
        {:ok, doc} = LoroEngine.doc_new()
        :ok = LoroEngine.text_insert(doc, "text", 0, "x")
      end,
      "nif_map_set" => fn ->
        {:ok, doc} = LoroEngine.doc_new()
        :ok = LoroEngine.map_set(doc, "map", "key", "val")
      end,
      "nif_snapshot_cycle" => fn ->
        {:ok, doc} = LoroEngine.doc_new()
        :ok = LoroEngine.text_insert(doc, "text", 0, "hello")
        {:ok, snap} = LoroEngine.doc_export_snapshot(doc)
        {:ok, doc2} = LoroEngine.doc_new()
        :ok = LoroEngine.doc_import(doc2, snap)
      end,
      "native_noop_baseline" => fn ->
        :ok
      end
    }, warmup: 2, time: 5)
  end
end
