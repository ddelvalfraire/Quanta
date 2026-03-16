defmodule Quanta.StateKind do
  @moduledoc """
  Describes how an actor's state is managed.
  """

  @type crdt_type :: :text | :map | :list | :tree | :counter

  @type t ::
          :opaque
          | {:crdt, crdt_type()}
          | {:schematized, String.t()}
          | {:authoritative, String.t() | nil}

  @valid_crdt_types %{
    "text" => :text,
    "map" => :map,
    "list" => :list,
    "tree" => :tree,
    "counter" => :counter
  }

  @spec parse(String.t()) :: {:ok, t()} | {:error, String.t()}
  def parse("opaque"), do: {:ok, :opaque}

  def parse("crdt:" <> subtype) when byte_size(subtype) > 0 do
    case Map.fetch(@valid_crdt_types, subtype) do
      {:ok, atom} -> {:ok, {:crdt, atom}}
      :error -> {:error, "unknown CRDT type: #{subtype}"}
    end
  end

  def parse("crdt:" <> _), do: {:error, "CRDT type must not be empty"}

  def parse("schematized:" <> ref) when byte_size(ref) > 0 do
    {:ok, {:schematized, ref}}
  end

  def parse("schematized:" <> _), do: {:error, "schematized ref must not be empty"}

  # Bare "authoritative" (no colon) maps to nil ref per the type definition.
  def parse("authoritative"), do: {:ok, {:authoritative, nil}}

  def parse("authoritative:" <> ref) when byte_size(ref) > 0 do
    {:ok, {:authoritative, ref}}
  end

  def parse("authoritative:" <> _), do: {:error, "authoritative ref must not be empty"}

  def parse(other), do: {:error, "unknown state kind: #{other}"}
end
