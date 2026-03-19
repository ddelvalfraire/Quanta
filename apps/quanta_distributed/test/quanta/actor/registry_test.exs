defmodule Quanta.Actor.RegistryTest do
  use ExUnit.Case, async: false

  alias Quanta.Actor.Registry
  alias Quanta.ActorId

  setup do
    :syn.add_node_to_scopes([:actors])
    :ok
  end

  defp make_actor_id(id) do
    %ActorId{namespace: "test", type: "counter", id: id}
  end

  describe "register/1 and lookup/1" do
    test "register + lookup returns {:ok, pid}" do
      actor_id = make_actor_id("reg-1")
      assert :ok = Registry.register(actor_id)
      assert {:ok, pid} = Registry.lookup(actor_id)
      assert pid == self()
    end
  end

  describe "lookup/1" do
    test "unregistered key returns :not_found" do
      actor_id = make_actor_id("nonexistent")
      assert :not_found = Registry.lookup(actor_id)
    end
  end

  describe "register/1" do
    test "double register from different process returns {:error, :already_registered}" do
      actor_id = make_actor_id("double-reg")
      assert :ok = Registry.register(actor_id)

      task =
        Task.async(fn ->
          Registry.register(actor_id)
        end)

      assert {:error, :already_registered} = Task.await(task)
    end
  end

  describe "register/2" do
    test "stores full distributed metadata" do
      actor_id = make_actor_id("meta-check")
      assert :ok = Registry.register(actor_id)

      {_pid, meta} = :syn.lookup(:actors, actor_id)
      assert meta.node == node()
      assert meta.type == "counter"
      assert is_integer(meta.nonce)
      assert is_integer(meta.activated_at)
      assert meta.draining == false
    end

    test "caller-supplied meta is merged" do
      actor_id = make_actor_id("extra-meta")
      assert :ok = Registry.register(actor_id, %{custom: "value"})

      {_pid, meta} = :syn.lookup(:actors, actor_id)
      assert meta.custom == "value"
      assert meta.draining == false
    end
  end

  describe "update_meta/2" do
    test "modifies stored metadata" do
      actor_id = make_actor_id("update-meta")
      assert :ok = Registry.register(actor_id)

      assert {:ok, {_pid, updated}} =
               Registry.update_meta(actor_id, fn meta -> %{meta | draining: true} end)

      assert updated.draining == true

      {_pid, meta} = :syn.lookup(:actors, actor_id)
      assert meta.draining == true
    end

    test "returns error for unregistered actor" do
      actor_id = make_actor_id("no-such-actor")

      assert {:error, :undefined} =
               Registry.update_meta(actor_id, fn meta -> meta end)
    end
  end

  describe "local_count/0 and cluster_count/0" do
    test "returns correct counts after registration" do
      before_local = Registry.local_count()
      before_cluster = Registry.cluster_count()

      actor_id = make_actor_id("count-test")
      assert :ok = Registry.register(actor_id)

      assert Registry.local_count() == before_local + 1
      assert Registry.cluster_count() == before_cluster + 1

      assert :ok = Registry.deregister(actor_id)

      assert Registry.local_count() == before_local
      assert Registry.cluster_count() == before_cluster
    end
  end

  describe "deregister/1" do
    test "deregister + lookup returns :not_found" do
      actor_id = make_actor_id("dereg-1")
      assert :ok = Registry.register(actor_id)
      assert {:ok, _} = Registry.lookup(actor_id)

      assert :ok = Registry.deregister(actor_id)
      assert :not_found = Registry.lookup(actor_id)
    end

    test "deregister unregistered key is idempotent" do
      actor_id = make_actor_id("never-registered")
      assert :ok = Registry.deregister(actor_id)
    end
  end
end
