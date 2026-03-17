defmodule Quanta.Actor.ManifestRegistryTest do
  use ExUnit.Case, async: false

  alias Quanta.Actor.ManifestRegistry
  alias Quanta.Manifest

  defp build_manifest(overrides \\ []) do
    attrs =
      Keyword.merge(
        [version: "1", type: "counter", namespace: "myapp"],
        overrides
      )

    struct!(Manifest, attrs)
  end

  setup do
    # Table is :protected — clear from the owning process
    :sys.replace_state(ManifestRegistry, fn state ->
      :ets.delete_all_objects(state)
      state
    end)

    :ok
  end

  describe "get/2" do
    test "returns :error for unknown type" do
      assert :error == ManifestRegistry.get("myapp", "unknown")
    end

    test "returns manifest after put" do
      m = build_manifest()
      assert :ok = ManifestRegistry.put(m)
      assert {:ok, ^m} = ManifestRegistry.get("myapp", "counter")
    end
  end

  describe "put/1" do
    test "registers a new manifest" do
      m = build_manifest()
      assert :ok = ManifestRegistry.put(m)
    end

    test "allows updating mutable fields" do
      m1 = build_manifest()
      assert :ok = ManifestRegistry.put(m1)

      m2 = build_manifest(lifecycle: %Manifest.Lifecycle{idle_timeout_ms: 600_000})
      assert :ok = ManifestRegistry.put(m2)
      assert {:ok, ^m2} = ManifestRegistry.get("myapp", "counter")
    end

    test "rejects changes to immutable state.kind" do
      m1 = build_manifest(state: %Manifest.State{kind: :opaque})
      assert :ok = ManifestRegistry.put(m1)

      m2 = build_manifest(state: %Manifest.State{kind: {:crdt, :text}})
      assert {:error, msg} = ManifestRegistry.put(m2)
      assert msg =~ "state.kind is immutable"
    end
  end

  describe "list_types/1" do
    test "returns empty list for unknown namespace" do
      assert [] == ManifestRegistry.list_types("unknown")
    end

    test "returns all types for a namespace" do
      m1 = build_manifest(type: "counter")
      m2 = build_manifest(type: "timer")
      assert :ok = ManifestRegistry.put(m1)
      assert :ok = ManifestRegistry.put(m2)

      types = ManifestRegistry.list_types("myapp")
      assert length(types) == 2
      assert Enum.any?(types, &(&1.type == "counter"))
      assert Enum.any?(types, &(&1.type == "timer"))
    end

    test "does not return types from other namespaces" do
      m1 = build_manifest(namespace: "ns1", type: "counter")
      m2 = build_manifest(namespace: "ns2", type: "counter")
      assert :ok = ManifestRegistry.put(m1)
      assert :ok = ManifestRegistry.put(m2)

      types = ManifestRegistry.list_types("ns1")
      assert length(types) == 1
      assert hd(types).namespace == "ns1"
    end
  end
end
