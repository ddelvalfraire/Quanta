defmodule Quanta.ActorIdTest do
  use ExUnit.Case, async: true

  alias Quanta.ActorId

  @valid %ActorId{namespace: "prod", type: "game", id: "room-123"}

  describe "validate/1" do
    test "accepts valid segments" do
      assert :ok == ActorId.validate(@valid)
    end

    test "accepts hyphens and underscores" do
      assert :ok == ActorId.validate(%ActorId{namespace: "my-ns", type: "my_type", id: "a-b_c"})
    end

    test "accepts all-numeric segments" do
      assert :ok == ActorId.validate(%ActorId{namespace: "123", type: "456", id: "789"})
    end

    test "accepts segments at max length" do
      ns = String.duplicate("a", 63)
      type = String.duplicate("b", 63)
      id = String.duplicate("c", 128)
      assert :ok == ActorId.validate(%ActorId{namespace: ns, type: type, id: id})
    end

    test "rejects empty namespace" do
      assert {:error, "namespace" <> _} = ActorId.validate(%{@valid | namespace: ""})
    end

    test "rejects empty type" do
      assert {:error, "type" <> _} = ActorId.validate(%{@valid | type: ""})
    end

    test "rejects empty id" do
      assert {:error, "id" <> _} = ActorId.validate(%{@valid | id: ""})
    end

    test "rejects dots in segments" do
      assert {:error, _} = ActorId.validate(%{@valid | namespace: "a.b"})
    end

    test "rejects wildcards" do
      assert {:error, _} = ActorId.validate(%{@valid | type: "*"})
      assert {:error, _} = ActorId.validate(%{@valid | type: ">"})
    end

    test "rejects spaces and special characters" do
      assert {:error, _} = ActorId.validate(%{@valid | id: "has space"})
      assert {:error, _} = ActorId.validate(%{@valid | id: "has@char"})
      assert {:error, _} = ActorId.validate(%{@valid | id: "has#char"})
    end

    test "rejects namespace longer than 63 chars" do
      assert {:error, "namespace" <> _} =
               ActorId.validate(%{@valid | namespace: String.duplicate("a", 64)})
    end

    test "rejects type longer than 63 chars" do
      assert {:error, "type" <> _} =
               ActorId.validate(%{@valid | type: String.duplicate("a", 64)})
    end

    test "rejects id longer than 128 chars" do
      assert {:error, "id" <> _} =
               ActorId.validate(%{@valid | id: String.duplicate("a", 129)})
    end
  end

  describe "to_nats_subject_fragment/1" do
    test "joins segments with dots" do
      assert "prod.game.room-123" == ActorId.to_nats_subject_fragment(@valid)
    end
  end

  describe "to_syn_key/1" do
    test "returns the struct itself" do
      assert @valid == ActorId.to_syn_key(@valid)
    end
  end
end
