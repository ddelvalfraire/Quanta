defmodule Quanta.Nifs.LoroEngineTest do
  use ExUnit.Case, async: true

  alias Quanta.Nifs.LoroEngine

  # Document lifecycle

  test "doc_new creates empty LoroDoc" do
    assert {:ok, doc} = LoroEngine.doc_new()
    assert is_reference(doc)
  end

  test "doc_new_with_peer_id creates doc with specific peer" do
    assert {:ok, doc} = LoroEngine.doc_new_with_peer_id(42)
    assert is_reference(doc)
  end

  test "doc_import + doc_export_snapshot roundtrip" do
    {:ok, doc1} = LoroEngine.doc_new()
    :ok = LoroEngine.text_insert(doc1, "text", 0, "hello")

    {:ok, snapshot} = LoroEngine.doc_export_snapshot(doc1)
    assert is_binary(snapshot)
    assert byte_size(snapshot) > 0

    {:ok, doc2} = LoroEngine.doc_new()
    :ok = LoroEngine.doc_import(doc2, snapshot)

    assert {:ok, "hello"} = LoroEngine.text_to_string(doc2, "text")
  end

  test "doc_export_shallow_snapshot produces output" do
    {:ok, doc} = LoroEngine.doc_new()
    :ok = LoroEngine.text_insert(doc, "text", 0, "content")

    {:ok, shallow} = LoroEngine.doc_export_shallow_snapshot(doc)
    assert is_binary(shallow)
    assert byte_size(shallow) > 0
  end

  test "doc_export_shallow_snapshot is not larger than full snapshot" do
    {:ok, doc} = LoroEngine.doc_new()

    for i <- 1..20 do
      :ok = LoroEngine.text_insert(doc, "text", 0, "line #{i}\n")
    end

    {:ok, full} = LoroEngine.doc_export_snapshot(doc)
    {:ok, shallow} = LoroEngine.doc_export_shallow_snapshot(doc)
    assert byte_size(shallow) <= byte_size(full)
  end

  test "doc_apply_delta merges concurrent edits" do
    {:ok, doc1} = LoroEngine.doc_new_with_peer_id(1)
    {:ok, doc2} = LoroEngine.doc_new_with_peer_id(2)

    :ok = LoroEngine.text_insert(doc1, "text", 0, "hello")
    :ok = LoroEngine.text_insert(doc2, "text", 0, "world")

    {:ok, updates1} = LoroEngine.doc_export_snapshot(doc1)
    {:ok, updates2} = LoroEngine.doc_export_snapshot(doc2)

    :ok = LoroEngine.doc_apply_delta(doc1, updates2)
    :ok = LoroEngine.doc_apply_delta(doc2, updates1)

    {:ok, text1} = LoroEngine.text_to_string(doc1, "text")
    {:ok, text2} = LoroEngine.text_to_string(doc2, "text")

    # Both docs converge to the same state
    assert text1 == text2
    # Both contain both insertions
    assert String.contains?(text1, "hello")
    assert String.contains?(text1, "world")
  end

  test "doc_get_value returns complete document as Erlang terms" do
    {:ok, doc} = LoroEngine.doc_new()
    :ok = LoroEngine.map_set(doc, "config", "name", "test")
    :ok = LoroEngine.map_set(doc, "config", "count", 42)

    {:ok, value} = LoroEngine.doc_get_value(doc)
    assert is_map(value)
    config = value["config"]
    assert config["name"] == "test"
    assert config["count"] == 42
  end

  test "doc_version returns opaque binary" do
    {:ok, doc} = LoroEngine.doc_new()
    :ok = LoroEngine.text_insert(doc, "text", 0, "x")

    {:ok, version} = LoroEngine.doc_version(doc)
    assert is_binary(version)
  end

  test "doc_export_updates_from with version vector" do
    {:ok, doc} = LoroEngine.doc_new()
    :ok = LoroEngine.text_insert(doc, "text", 0, "first")

    # Sync doc2 from doc1's current state
    {:ok, snapshot} = LoroEngine.doc_export_snapshot(doc)
    {:ok, doc2} = LoroEngine.doc_new()
    :ok = LoroEngine.doc_import(doc2, snapshot)
    {:ok, v1} = LoroEngine.doc_version(doc)

    # Make more changes on doc
    :ok = LoroEngine.text_insert(doc, "text", 5, " second")

    # Export only the updates since v1
    {:ok, updates} = LoroEngine.doc_export_updates_from(doc, v1)
    assert is_binary(updates)
    assert byte_size(updates) > 0

    # Apply incremental updates to doc2
    :ok = LoroEngine.doc_import(doc2, updates)
    assert {:ok, "first second"} = LoroEngine.text_to_string(doc2, "text")
  end

  test "doc_state_size returns byte count" do
    {:ok, doc} = LoroEngine.doc_new()
    {:ok, size1} = LoroEngine.doc_state_size(doc)
    assert is_integer(size1)
    assert size1 > 0

    :ok = LoroEngine.text_insert(doc, "text", 0, String.duplicate("x", 1000))
    {:ok, size2} = LoroEngine.doc_state_size(doc)
    assert size2 > size1
  end

  # Text container

  test "text insert/delete/to_string operations" do
    {:ok, doc} = LoroEngine.doc_new()
    :ok = LoroEngine.text_insert(doc, "text", 0, "hello world")
    assert {:ok, "hello world"} = LoroEngine.text_to_string(doc, "text")

    :ok = LoroEngine.text_delete(doc, "text", 5, 6)
    assert {:ok, "hello"} = LoroEngine.text_to_string(doc, "text")
  end

  test "text_length returns Unicode character count" do
    {:ok, doc} = LoroEngine.doc_new()
    :ok = LoroEngine.text_insert(doc, "text", 0, "héllo")
    {:ok, len} = LoroEngine.text_length(doc, "text")
    assert len == 5
  end

  test "text_mark with Peritext expand modes" do
    {:ok, doc} = LoroEngine.doc_new()
    :ok = LoroEngine.configure_text_style(doc, "bold", "after")
    :ok = LoroEngine.text_insert(doc, "text", 0, "hello world")
    :ok = LoroEngine.text_mark(doc, "text", 0, 5, "bold", true)

    {:ok, value} = LoroEngine.doc_get_value(doc)
    assert is_map(value)
  end

  test "configure_text_style rejects invalid expand mode" do
    {:ok, doc} = LoroEngine.doc_new()
    assert {:error, msg} = LoroEngine.configure_text_style(doc, "bold", "invalid")
    assert msg =~ "invalid expand type"
  end

  # Map container

  test "map set/get/delete operations" do
    {:ok, doc} = LoroEngine.doc_new()
    :ok = LoroEngine.map_set(doc, "settings", "theme", "dark")
    assert {:ok, "dark"} = LoroEngine.map_get(doc, "settings", "theme")

    :ok = LoroEngine.map_delete(doc, "settings", "theme")
    assert {:error, _} = LoroEngine.map_get(doc, "settings", "theme")
  end

  test "map supports various value types" do
    {:ok, doc} = LoroEngine.doc_new()
    :ok = LoroEngine.map_set(doc, "m", "int", 42)
    :ok = LoroEngine.map_set(doc, "m", "float", 3.14)
    :ok = LoroEngine.map_set(doc, "m", "str", "hello")
    :ok = LoroEngine.map_set(doc, "m", "bool", true)
    :ok = LoroEngine.map_set(doc, "m", "null", nil)

    assert {:ok, 42} = LoroEngine.map_get(doc, "m", "int")
    assert {:ok, 3.14} = LoroEngine.map_get(doc, "m", "float")
    assert {:ok, "hello"} = LoroEngine.map_get(doc, "m", "str")
    assert {:ok, true} = LoroEngine.map_get(doc, "m", "bool")
    assert {:ok, nil} = LoroEngine.map_get(doc, "m", "null")
  end

  # List container

  test "list insert/get/delete/length operations" do
    {:ok, doc} = LoroEngine.doc_new()
    :ok = LoroEngine.list_insert(doc, "items", 0, "a")
    :ok = LoroEngine.list_insert(doc, "items", 1, "b")
    :ok = LoroEngine.list_insert(doc, "items", 2, "c")

    assert {:ok, 3} = LoroEngine.list_length(doc, "items")
    assert {:ok, "a"} = LoroEngine.list_get(doc, "items", 0)
    assert {:ok, "b"} = LoroEngine.list_get(doc, "items", 1)
    assert {:ok, "c"} = LoroEngine.list_get(doc, "items", 2)

    :ok = LoroEngine.list_delete(doc, "items", 1, 1)
    assert {:ok, 2} = LoroEngine.list_length(doc, "items")
    assert {:ok, "c"} = LoroEngine.list_get(doc, "items", 1)
  end

  test "list_get out of bounds returns error" do
    {:ok, doc} = LoroEngine.doc_new()
    assert {:error, _} = LoroEngine.list_get(doc, "items", 0)
  end

  # Tree container

  test "tree create/move/delete operations" do
    {:ok, doc} = LoroEngine.doc_new()
    {:ok, node1} = LoroEngine.tree_create_node(doc, "tree")
    {:ok, node2} = LoroEngine.tree_create_node(doc, "tree")

    assert is_binary(node1)
    assert String.contains?(node1, ":")

    # Move node2 under node1
    :ok = LoroEngine.tree_move(doc, "tree", node2, node1)

    # Move node2 back to root
    :ok = LoroEngine.tree_move(doc, "tree", node2, nil)

    # Delete node2
    :ok = LoroEngine.tree_delete(doc, "tree", node2)
  end

  # Cursor

  test "cursor at/pos roundtrip survives edits" do
    {:ok, doc} = LoroEngine.doc_new()
    :ok = LoroEngine.text_insert(doc, "text", 0, "hello world")

    # Get cursor at position 5 (the space)
    {:ok, cursor} = LoroEngine.cursor_at(doc, "text", 5, 0)
    assert is_binary(cursor)

    # Insert text before the cursor
    :ok = LoroEngine.text_insert(doc, "text", 0, "prefix ")

    # Cursor should now point to the shifted position
    {:ok, pos} = LoroEngine.cursor_pos(doc, cursor)
    assert pos == 12  # "prefix " (7 chars) + original 5 = 12
  end

  # Concurrent access via Mutex

  test "rapid sequential calls are correct" do
    {:ok, doc} = LoroEngine.doc_new()

    for i <- 0..99 do
      :ok = LoroEngine.map_set(doc, "data", "key_#{i}", i)
    end

    for i <- 0..99 do
      assert {:ok, ^i} = LoroEngine.map_get(doc, "data", "key_#{i}")
    end
  end

  # Error handling

  test "doc_import with invalid bytes returns error" do
    {:ok, doc} = LoroEngine.doc_new()
    assert {:error, _} = LoroEngine.doc_import(doc, <<0, 1, 2, 3>>)
  end

  test "doc_export_updates_from with empty version vector exports all" do
    {:ok, doc} = LoroEngine.doc_new()
    :ok = LoroEngine.text_insert(doc, "text", 0, "hello")

    {:ok, updates} = LoroEngine.doc_export_updates_from(doc, <<>>)
    assert is_binary(updates)
    assert byte_size(updates) > 0

    {:ok, doc2} = LoroEngine.doc_new()
    :ok = LoroEngine.doc_import(doc2, updates)
    assert {:ok, "hello"} = LoroEngine.text_to_string(doc2, "text")
  end

  test "doc_export_updates_from with malformed version vector returns error" do
    {:ok, doc} = LoroEngine.doc_new()
    assert {:error, msg} = LoroEngine.doc_export_updates_from(doc, <<1, 2>>)
    assert msg =~ "version vector too short"
  end

  # All Peritext expand modes

  test "configure_text_style supports all four expand modes" do
    {:ok, doc} = LoroEngine.doc_new()
    :ok = LoroEngine.configure_text_style(doc, "bold", "after")
    :ok = LoroEngine.configure_text_style(doc, "code", "none")
    :ok = LoroEngine.configure_text_style(doc, "italic", "before")
    :ok = LoroEngine.configure_text_style(doc, "underline", "both")

    :ok = LoroEngine.text_insert(doc, "text", 0, "hello world")
    :ok = LoroEngine.text_mark(doc, "text", 0, 5, "bold", true)
    :ok = LoroEngine.text_mark(doc, "text", 0, 5, "code", true)
    :ok = LoroEngine.text_mark(doc, "text", 6, 11, "italic", true)
    :ok = LoroEngine.text_mark(doc, "text", 6, 11, "underline", true)

    {:ok, value} = LoroEngine.doc_get_value(doc)
    assert is_map(value)
  end

  # Concurrent access from multiple tasks

  test "concurrent access from multiple tasks is safe" do
    {:ok, doc} = LoroEngine.doc_new()

    tasks =
      for i <- 0..19 do
        Task.async(fn ->
          for j <- 0..9 do
            :ok = LoroEngine.map_set(doc, "data", "t#{i}_k#{j}", i * 10 + j)
          end
        end)
      end

    Task.await_many(tasks, 5_000)

    {:ok, value} = LoroEngine.doc_get_value(doc)
    data = value["data"]
    assert map_size(data) == 200
  end
end
