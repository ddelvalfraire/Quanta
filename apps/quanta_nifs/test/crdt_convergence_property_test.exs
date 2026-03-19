defmodule Quanta.Nifs.CrdtConvergencePropertyTest do
  @moduledoc """
  Property test P3: CRDT convergence.

  Two independent LoroDoc replicas apply random operations, then cross-merge.
  After merging, both documents must have identical state (strong eventual consistency).

  Uses position-0 inserts for text to avoid invalid position errors.
  Keeps iterations modest (100 cases, 5-15 ops) to avoid NIF resource pressure.
  """

  use ExUnit.Case, async: true
  use PropCheck

  alias Quanta.Nifs.LoroEngine

  @moduletag :property

  # ── Generators ──────────────────────────────────────────────────────

  @map_keys ["title", "author", "status", "count", "flag"]

  defp text_content do
    let chars <- non_empty(list(integer(?a, ?z))) do
      List.to_string(chars)
    end
  end

  defp map_key, do: oneof(@map_keys)

  defp map_value do
    oneof([
      text_content(),
      integer(),
      boolean()
    ])
  end

  defp crdt_op do
    frequency([
      {3, {:text_insert, text_content()}},
      {3, {:map_set, map_key(), map_value()}}
    ])
  end

  defp op_list do
    let len <- integer(5, 15) do
      vector(len, crdt_op())
    end
  end

  # ── Helpers ─────────────────────────────────────────────────────────

  defp apply_op(doc, {:text_insert, text}) do
    # Always insert at position 0 to avoid invalid position errors
    LoroEngine.text_insert(doc, "text", 0, text)
  end

  defp apply_op(doc, {:map_set, key, value}) do
    LoroEngine.map_set(doc, "map", key, value)
  end

  defp apply_ops(doc, ops) do
    Enum.each(ops, fn op -> :ok = apply_op(doc, op) end)
  end

  # ── Properties ──────────────────────────────────────────────────────

  property "two replicas converge after cross-merge", [numtests: 100] do
    forall {ops1, ops2} <- {op_list(), op_list()} do
      # Create two independent replicas with different peer IDs
      {:ok, doc1} = LoroEngine.doc_new_with_peer_id(1)
      {:ok, doc2} = LoroEngine.doc_new_with_peer_id(2)

      # Each replica applies its own operations independently
      apply_ops(doc1, ops1)
      apply_ops(doc2, ops2)

      # Get initial version of each before export (empty version = export all)
      {:ok, v1_initial} = LoroEngine.doc_version(doc1)
      {:ok, v2_initial} = LoroEngine.doc_version(doc2)

      # Export full snapshots and cross-merge
      {:ok, snapshot1} = LoroEngine.doc_export_snapshot(doc1)
      {:ok, snapshot2} = LoroEngine.doc_export_snapshot(doc2)

      :ok = LoroEngine.doc_apply_delta(doc1, snapshot2)
      :ok = LoroEngine.doc_apply_delta(doc2, snapshot1)

      # Both replicas must now have identical state
      {:ok, val1} = LoroEngine.doc_get_value(doc1)
      {:ok, val2} = LoroEngine.doc_get_value(doc2)

      val1 == val2
    end
  end

  property "incremental delta merge also converges", [numtests: 100] do
    forall {ops1, ops2} <- {op_list(), op_list()} do
      {:ok, doc1} = LoroEngine.doc_new_with_peer_id(10)
      {:ok, doc2} = LoroEngine.doc_new_with_peer_id(20)

      # Record version before applying ops
      {:ok, v1_before} = LoroEngine.doc_version(doc1)
      {:ok, v2_before} = LoroEngine.doc_version(doc2)

      apply_ops(doc1, ops1)
      apply_ops(doc2, ops2)

      # Export only the new updates (delta since empty version)
      {:ok, delta1} = LoroEngine.doc_export_updates_from(doc1, v1_before)
      {:ok, delta2} = LoroEngine.doc_export_updates_from(doc2, v2_before)

      # Cross-apply deltas
      :ok = LoroEngine.doc_apply_delta(doc1, delta2)
      :ok = LoroEngine.doc_apply_delta(doc2, delta1)

      {:ok, val1} = LoroEngine.doc_get_value(doc1)
      {:ok, val2} = LoroEngine.doc_get_value(doc2)

      val1 == val2
    end
  end

  property "snapshot import is idempotent", [numtests: 50] do
    forall ops <- op_list() do
      {:ok, doc} = LoroEngine.doc_new_with_peer_id(99)
      apply_ops(doc, ops)

      {:ok, snapshot} = LoroEngine.doc_export_snapshot(doc)
      {:ok, val_before} = LoroEngine.doc_get_value(doc)

      # Importing own snapshot should be a no-op
      :ok = LoroEngine.doc_apply_delta(doc, snapshot)
      {:ok, val_after} = LoroEngine.doc_get_value(doc)

      val_before == val_after
    end
  end
end
