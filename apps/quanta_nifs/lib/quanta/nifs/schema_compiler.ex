defmodule Quanta.Nifs.SchemaCompiler do
  @moduledoc false

  alias Quanta.Nifs.Native

  @spec compile(wit_source :: String.t(), type_name :: String.t()) ::
          {:ok, reference(), [String.t()]} | {:error, String.t()}
  def compile(wit_source, type_name) when is_binary(wit_source) and is_binary(type_name) do
    Native.schema_compile(wit_source, type_name)
  end

  @spec export(schema :: reference()) :: {:ok, binary()}
  def export(schema) do
    Native.schema_export(schema)
  end
end
