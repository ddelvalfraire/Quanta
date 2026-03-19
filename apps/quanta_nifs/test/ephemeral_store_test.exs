defmodule Quanta.Nifs.EphemeralStoreTest do
  use ExUnit.Case, async: true

  alias Quanta.Nifs.EphemeralStore

  test "new creates EphemeralStore with default TTL" do
    assert {:ok, store} = EphemeralStore.new()
    assert is_reference(store)
  end

  test "new creates EphemeralStore with custom TTL" do
    assert {:ok, store} = EphemeralStore.new(5_000)
    assert is_reference(store)
  end

  test "set/get roundtrip" do
    {:ok, store} = EphemeralStore.new()
    :ok = EphemeralStore.set(store, "user1", <<1, 2, 3>>)
    assert {:ok, <<1, 2, 3>>} = EphemeralStore.get(store, "user1")
  end

  test "get returns :not_found for missing key" do
    {:ok, store} = EphemeralStore.new()
    assert :not_found = EphemeralStore.get(store, "nonexistent")
  end

  test "set overwrites existing value" do
    {:ok, store} = EphemeralStore.new()
    :ok = EphemeralStore.set(store, "user1", <<1, 2, 3>>)
    :ok = EphemeralStore.set(store, "user1", <<4, 5, 6>>)
    assert {:ok, <<4, 5, 6>>} = EphemeralStore.get(store, "user1")
  end

  test "delete removes entry" do
    {:ok, store} = EphemeralStore.new()
    :ok = EphemeralStore.set(store, "user1", <<1, 2, 3>>)
    :ok = EphemeralStore.delete(store, "user1")
    assert :not_found = EphemeralStore.get(store, "user1")
  end

  test "delete on missing key is no-op" do
    {:ok, store} = EphemeralStore.new()
    :ok = EphemeralStore.delete(store, "nonexistent")
  end

  test "get_all returns all current entries" do
    {:ok, store} = EphemeralStore.new()
    :ok = EphemeralStore.set(store, "user1", <<1>>)
    :ok = EphemeralStore.set(store, "user2", <<2>>)
    :ok = EphemeralStore.set(store, "user3", <<3>>)

    {:ok, all} = EphemeralStore.get_all(store)
    assert map_size(all) == 3
    assert all["user1"] == <<1>>
    assert all["user2"] == <<2>>
    assert all["user3"] == <<3>>
  end

  test "get_all returns empty map for empty store" do
    {:ok, store} = EphemeralStore.new()
    {:ok, all} = EphemeralStore.get_all(store)
    assert all == %{}
  end

  test "keys returns all active keys" do
    {:ok, store} = EphemeralStore.new()
    :ok = EphemeralStore.set(store, "user1", <<1>>)
    :ok = EphemeralStore.set(store, "user2", <<2>>)

    {:ok, keys} = EphemeralStore.keys(store)
    assert Enum.sort(keys) == ["user1", "user2"]
  end

  test "keys returns empty list for empty store" do
    {:ok, store} = EphemeralStore.new()
    {:ok, keys} = EphemeralStore.keys(store)
    assert keys == []
  end

  test "encode/apply_encoded roundtrip syncs two stores" do
    {:ok, store1} = EphemeralStore.new()
    {:ok, store2} = EphemeralStore.new()

    :ok = EphemeralStore.set(store1, "user1", <<10, 20, 30>>)
    {:ok, encoded} = EphemeralStore.encode(store1, "user1")
    assert is_binary(encoded)
    assert byte_size(encoded) > 0

    :ok = EphemeralStore.apply_encoded(store2, encoded)
    assert {:ok, <<10, 20, 30>>} = EphemeralStore.get(store2, "user1")
  end

  test "encode for missing key returns a binary" do
    {:ok, store} = EphemeralStore.new()
    assert {:ok, bytes} = EphemeralStore.encode(store, "nonexistent")
    assert is_binary(bytes)
  end

  test "encode returns partial update (only specified key)" do
    {:ok, store1} = EphemeralStore.new()
    {:ok, store2} = EphemeralStore.new()

    :ok = EphemeralStore.set(store1, "user1", <<1>>)
    :ok = EphemeralStore.set(store1, "user2", <<2>>)

    {:ok, encoded} = EphemeralStore.encode(store1, "user1")
    :ok = EphemeralStore.apply_encoded(store2, encoded)

    assert {:ok, <<1>>} = EphemeralStore.get(store2, "user1")
    assert :not_found = EphemeralStore.get(store2, "user2")
  end

  test "encode_all syncs full state" do
    {:ok, store1} = EphemeralStore.new()
    {:ok, store2} = EphemeralStore.new()

    :ok = EphemeralStore.set(store1, "user1", <<1>>)
    :ok = EphemeralStore.set(store1, "user2", <<2>>)

    {:ok, encoded} = EphemeralStore.encode_all(store1)
    :ok = EphemeralStore.apply_encoded(store2, encoded)

    assert {:ok, <<1>>} = EphemeralStore.get(store2, "user1")
    assert {:ok, <<2>>} = EphemeralStore.get(store2, "user2")
  end

  test "encode after delete does not crash and produces valid bytes" do
    {:ok, store1} = EphemeralStore.new()
    {:ok, store2} = EphemeralStore.new()

    :ok = EphemeralStore.set(store1, "user1", <<1, 2, 3>>)

    :ok = EphemeralStore.delete(store1, "user1")
    {:ok, encoded} = EphemeralStore.encode(store1, "user1")
    assert is_binary(encoded)

    :ok = EphemeralStore.apply_encoded(store2, encoded)
  end

  test "apply_encoded with empty bytes returns error" do
    {:ok, store} = EphemeralStore.new()
    assert {:error, _reason} = EphemeralStore.apply_encoded(store, <<>>)
  end

  test "entries expire after TTL" do
    {:ok, store} = EphemeralStore.new(100)
    :ok = EphemeralStore.set(store, "user1", <<1, 2, 3>>)
    assert {:ok, <<1, 2, 3>>} = EphemeralStore.get(store, "user1")

    Process.sleep(200)
    assert :not_found = EphemeralStore.get(store, "user1")
  end

  test "multiple stores are independent" do
    {:ok, store1} = EphemeralStore.new()
    {:ok, store2} = EphemeralStore.new()

    :ok = EphemeralStore.set(store1, "key", <<1>>)
    assert :not_found = EphemeralStore.get(store2, "key")
  end

  test "rapid sequential operations are correct" do
    {:ok, store} = EphemeralStore.new()

    for i <- 0..99 do
      :ok = EphemeralStore.set(store, "key_#{i}", <<i::8>>)
    end

    for i <- 0..99 do
      assert {:ok, <<^i::8>>} = EphemeralStore.get(store, "key_#{i}")
    end
  end

  test "concurrent access from multiple tasks is safe" do
    {:ok, store} = EphemeralStore.new()

    tasks =
      for i <- 0..19 do
        Task.async(fn ->
          for j <- 0..9 do
            :ok = EphemeralStore.set(store, "t#{i}_k#{j}", <<i::8, j::8>>)
          end
        end)
      end

    Task.await_many(tasks, 5_000)

    {:ok, all} = EphemeralStore.get_all(store)
    assert map_size(all) == 200
  end
end
