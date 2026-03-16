defmodule Quanta.StateKindTest do
  use ExUnit.Case, async: true

  alias Quanta.StateKind

  describe "parse/1 valid inputs" do
    test "opaque" do
      assert {:ok, :opaque} == StateKind.parse("opaque")
    end

    test "crdt types" do
      assert {:ok, {:crdt, :text}} == StateKind.parse("crdt:text")
      assert {:ok, {:crdt, :map}} == StateKind.parse("crdt:map")
      assert {:ok, {:crdt, :list}} == StateKind.parse("crdt:list")
      assert {:ok, {:crdt, :tree}} == StateKind.parse("crdt:tree")
      assert {:ok, {:crdt, :counter}} == StateKind.parse("crdt:counter")
    end

    test "schematized with ref" do
      assert {:ok, {:schematized, "v1.schema"}} == StateKind.parse("schematized:v1.schema")
    end

    test "authoritative with ref" do
      assert {:ok, {:authoritative, "v1"}} == StateKind.parse("authoritative:v1")
    end

    test "authoritative without ref" do
      assert {:ok, {:authoritative, nil}} == StateKind.parse("authoritative")
    end
  end

  describe "parse/1 invalid inputs" do
    test "unknown crdt type" do
      assert {:error, "unknown CRDT type: invalid"} == StateKind.parse("crdt:invalid")
    end

    test "empty crdt type" do
      assert {:error, "CRDT type must not be empty"} == StateKind.parse("crdt:")
    end

    test "schematized with empty ref" do
      assert {:error, "schematized ref must not be empty"} == StateKind.parse("schematized:")
    end

    test "authoritative with empty ref" do
      assert {:error, "authoritative ref must not be empty"} ==
               StateKind.parse("authoritative:")
    end

    test "empty string" do
      assert {:error, _} = StateKind.parse("")
    end

    test "case sensitive" do
      assert {:error, _} = StateKind.parse("OPAQUE")
      assert {:error, _} = StateKind.parse("Opaque")
    end

    test "unknown kind" do
      assert {:error, "unknown state kind: garbage"} == StateKind.parse("garbage")
    end
  end
end
