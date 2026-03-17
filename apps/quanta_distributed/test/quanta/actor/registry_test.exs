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
