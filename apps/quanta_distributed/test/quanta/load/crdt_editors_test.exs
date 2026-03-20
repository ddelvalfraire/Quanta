defmodule Quanta.Load.CrdtEditorsTest do
  @moduledoc """
  LD2: Concurrent CRDT editors load test.

  Simulates 50 concurrent editors writing to the same Loro document via the
  actor system. Validates that all edits converge and throughput meets SLOs.
  """

  use ExUnit.Case, async: false

  @moduletag :load
  @moduletag timeout: 600_000

  @editor_count 50
  @edits_per_editor 1_000

  # SLO: 50 concurrent editors sustain > 5K edits/sec aggregate
  # SLO: all replicas converge to identical state within 5s of last edit
  # SLO: no edit lost — final doc length == total chars inserted

  test "50 editors concurrently editing the same document" do
    # TODO: Create a single actor with a Loro doc (text container)
    # TODO: Spawn @editor_count tasks, each sending @edits_per_editor text_insert
    #       commands with unique content (e.g., editor_id + sequence number)
    # TODO: Await all tasks, record wall-clock time
    # TODO: Compute edits/sec = (@editor_count * @edits_per_editor) / elapsed
    # TODO: Assert edits/sec > 5_000
    # TODO: Read final doc state, assert all inserts are present
  end

  test "concurrent editors with mixed insert/delete operations" do
    # TODO: Same setup as above but with 70% inserts, 30% deletes
    # TODO: Assert no crashes, all operations applied without error
    # TODO: Assert final doc state is consistent across a snapshot export/import
  end
end
